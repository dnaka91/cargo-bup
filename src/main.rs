use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    hash::{Hash, Hasher},
    path::PathBuf,
};

use anyhow::{Context, Result};
use cargo::{InstallInfo, PackageId};
use clap::{Args, IntoApp, Parser, Subcommand};
use clap_complete::Shell;
use crates_index::Index;
use owo_colors::OwoColorize;
use semver::Version;
use siphasher::sip::SipHasher24;

use crate::{
    cargo::{CrateListingV2, SourceKind},
    table::Table,
};

mod cargo;
mod table;

/// Update your cargo-installed binaries.
#[derive(Parser)]
#[clap(about, author, version, bin_name = "cargo")]
enum Opt {
    /// Update your cargo-installed binaries.
    Bup(Command),
}

#[derive(Args)]
struct Command {
    #[clap(flatten)]
    select_args: SelectArgs,
    #[clap(subcommand)]
    subcmd: Option<Subcmd>,
}

#[derive(Args)]
struct SelectArgs {
    /// Include pre-releases in updates.
    #[clap(long)]
    pre: bool,
    /// Include crates installed from git repos (potentially slow).
    ///
    /// To find updates, each crate's local Git repository is updated against the remote repo.
    #[clap(long)]
    git: bool,
    /// Include crates installed from local paths (potentially slow).
    ///
    /// There is no way of checking the freshness for a crate that was installed locally, so cargo
    /// is invoked for each entry unconditionally.
    #[clap(long)]
    path: bool,
}

#[derive(Subcommand)]
enum Subcmd {
    /// Generate shell completions, writing them to the standard output.
    Completions {
        /// The shell type to generate completions for.
        #[clap(value_enum)]
        shell: Shell,
    },
}

fn main() -> Result<()> {
    let Opt::Bup(cmd) = Opt::parse();

    if let Some(Subcmd::Completions { shell }) = cmd.subcmd {
        clap_complete::generate(
            shell,
            &mut Opt::command(),
            env!("CARGO_PKG_NAME"),
            &mut std::io::stdout().lock(),
        );
        return Ok(());
    }

    let info = load_crate_state()?;
    let index = load_index()?;
    let updates = collect_updates(info, &index, &cmd.select_args)?;

    println!();

    print_registry_updates(&updates.registry);
    print_git_updates(&updates.git, cmd.select_args.git);
    print_path_updates(&updates.path, cmd.select_args.path);

    println!();

    install_registry_updates(updates.registry.into_iter());
    install_git_updates(updates.git.into_iter());
    install_path_updates(updates.path.into_iter());

    Ok(())
}

fn load_crate_state() -> Result<CrateListingV2> {
    let _guard = progress(format_args!(
        "{} loading {}",
        "[1/3]".bold(),
        "crate state".green().bold()
    ));

    let home = cargo_home()?;
    let path = home.join(".crates2.json");

    let file = File::open(path)?;
    let info = serde_json::from_reader::<_, CrateListingV2>(file)?;

    Ok(info)
}

fn load_index() -> Result<Index> {
    let _guard = progress(format_args!(
        "{} loading {}",
        "[2/3]".bold(),
        "crates.io index".green().bold()
    ));

    let mut index = Index::new_cargo_default()?;
    index.update()?;

    Ok(index)
}

fn collect_updates(info: CrateListingV2, index: &Index, args: &SelectArgs) -> Result<Updates> {
    let _guard = progress(format_args!(
        "{} collecting {}",
        "[3/3]".bold(),
        "updates".green().bold()
    ));
    let mut updates = Updates::default();

    for (package, info) in info.installs.into_iter() {
        match package.source_id.kind {
            SourceKind::Git(_) => {
                if !args.git {
                    continue;
                }

                let ident = package
                    .source_id
                    .canonical_url
                    .0
                    .path_segments()
                    .and_then(|s| s.last())
                    .unwrap_or("");

                let ident = if ident.is_empty() { "_empty" } else { ident };

                let mut hasher = SipHasher24::new();
                package.source_id.canonical_url.hash(&mut hasher);
                let hash = hex::encode(hasher.finish().to_le_bytes());
                let path = format!("{ident}-{hash}");

                updates.git.insert(package, path);
            }
            SourceKind::Path => {
                if !args.path {
                    continue;
                }

                updates.path.insert(package, ());
            }
            SourceKind::Registry => {
                let krate = index
                    .crate_(&package.name)
                    .context("failed finding package")?;

                let latest = Version::parse(krate.latest_version().version())?;

                if !latest.pre.is_empty() && !args.pre {
                    continue;
                }

                if latest > package.version {
                    if latest.major != package.version.major {}

                    updates.registry.insert(package, (latest, info));
                }
            }
        }
    }

    Ok(updates)
}

#[derive(Default)]
struct Updates {
    registry: BTreeMap<PackageId, (Version, InstallInfo)>,
    git: BTreeMap<PackageId, String>,
    path: BTreeMap<PackageId, ()>,
}

fn print_registry_updates(updates: &BTreeMap<PackageId, (Version, InstallInfo)>) {
    if updates.is_empty() {
        println!("no {} crate updates", "registry".green());
    } else {
        let registry = updates
            .iter()
            .map(|(pkg, (latest, _))| (pkg.name.as_str(), &pkg.version, latest))
            .collect::<Table>();

        println!("<<< Updates from the {} >>>", "registry".green());
        println!("{registry}\n");
    }
}

fn print_git_updates(updates: &BTreeMap<PackageId, String>, enabled: bool) {
    if !enabled {
        println!(
            "{} crate updates {}",
            "git".green(),
            "disabled".yellow().bold()
        );
    } else if updates.is_empty() {
        println!("no {} crate updates", "git".green());
    } else {
        let gits = updates
            .iter()
            .map(|(pkg, path)| (pkg.name.as_str(), path.as_str()))
            .collect::<BTreeMap<_, _>>();

        let width = gits
            .keys()
            .max_by_key(|k| k.len())
            .map(|k| k.len())
            .unwrap_or(4);

        println!("<<< Updates from {} >>>", "git".green());

        println!("\n{:width$}  Git Paths", "Name");
        println!("{}", "─".repeat(width * 2 + 2 + 1 + 16));

        for (name, git) in gits {
            println!("{name:width$}  {git}");
        }

        println!("\n");
    }
}

fn print_path_updates(updates: &BTreeMap<PackageId, ()>, enabled: bool) {
    if !enabled {
        println!(
            "{} crate updates {}",
            "local path".green(),
            "disabled".yellow().bold()
        );
    } else if updates.is_empty() {
        println!("no {} crates", "local path".green());
    } else {
        println!("<<< Updates from {} >>>", "local paths".green());

        let paths = updates
            .iter()
            .map(|(pkg, ())| pkg.name.as_str())
            .collect::<BTreeSet<_>>();

        let width = paths
            .iter()
            .max_by_key(|n| n.len())
            .map(|n| n.len())
            .unwrap_or(4);

        println!("\nName");
        println!("{}", "─".repeat(width));

        for name in paths {
            println!("{name}");
        }
    }
}

fn cargo_home() -> Result<PathBuf> {
    match std::env::var_os("CARGO_HOME") {
        Some(home) => Ok(home.into()),
        None => Ok(dirs::home_dir()
            .context("failed finding home directory")?
            .join(".cargo")),
    }
}

fn install_registry_updates(
    updates: impl ExactSizeIterator<Item = (PackageId, (Version, InstallInfo))>,
) {
    let count = updates.len();

    if count > 0 {
        println!(
            "start installing {} {} updates",
            count.blue().bold(),
            "registry".green().bold()
        );

        for (i, (pkg, (latest, info))) in updates.enumerate() {
            println!(
                "\n{} updating {} from {} to {}",
                format_args!("[{}/{}]", i + 1, count).bold(),
                pkg.name.green().bold(),
                pkg.version.blue().bold(),
                latest.blue().bold()
            );

            if let Err(e) = cargo_install_from_registry(&pkg.name, &latest, &info) {
                println!(
                    "installing {} {}:\n{e}",
                    pkg.name.green().bold(),
                    "failed".red().bold()
                )
            }
        }
    }
}

fn install_git_updates(updates: impl ExactSizeIterator<Item = (PackageId, String)>) {
    let count = updates.len();

    if count > 0 {
        println!(
            "start installing {} {} updates",
            count.blue().bold(),
            "git".green().bold()
        );
    }
}

fn install_path_updates(updates: impl ExactSizeIterator<Item = (PackageId, ())>) {
    let count = updates.len();

    if count > 0 {
        println!(
            "start installing {} {} updates",
            count.blue().bold(),
            "local path".green().bold()
        );
    }
}

fn cargo_install_from_registry(name: &str, version: &Version, info: &InstallInfo) -> Result<()> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(&["install", name]);

    cmd.arg("--version");
    cmd.arg(version.to_string());

    for bin in &info.bins {
        cmd.args(&["--bin", bin]);
    }

    if info.all_features {
        cmd.arg("--all-features");
    } else if !info.features.is_empty() {
        cmd.arg("--features");
        cmd.arg(info.features.iter().fold(String::new(), |mut s, f| {
            if !s.is_empty() {
                s.push(',');
            }
            s.push_str(f);
            s
        }));
    }

    if info.no_default_features {
        cmd.arg("--no-default-features");
    }

    if !info.profile.is_empty() {
        cmd.args(&["--profile", &info.profile]);
    }

    if !cmd.status()?.success() {
        eprintln!("failed installing `{name}`");
    }

    Ok(())
}

struct ProgressGuard;

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        println!("done");
    }
}

fn progress(msg: fmt::Arguments) -> ProgressGuard {
    print!("{}... ", msg);
    ProgressGuard
}
