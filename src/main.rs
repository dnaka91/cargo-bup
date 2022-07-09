use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    hash::{Hash, Hasher},
};

use anyhow::{bail, Context, Result};
use cargo::{InstallInfo, PackageId};
use clap::{Args, IntoApp, Parser, Subcommand};
use clap_complete::Shell;
use crates_index::Index;
use git2::{Commit, Oid, Repository};
use owo_colors::OwoColorize;
use semver::Version;
use siphasher::sip::SipHasher24;

use crate::{
    cargo::{CrateListingV2, GitReference, SourceKind},
    table::{GitTable, RegistryTable},
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
    #[clap(short = 'n', long)]
    dry_run: bool,
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

    if !cmd.dry_run {
        install_registry_updates(updates.registry.into_iter());
        install_git_updates(updates.git.into_iter());
        install_path_updates(updates.path.into_iter());
    }

    Ok(())
}

fn load_crate_state() -> Result<CrateListingV2> {
    let _guard = progress(format_args!(
        "{} loading {}",
        "[1/3]".bold(),
        "crate state".green().bold()
    ));

    let home = home::cargo_home()?;
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
            SourceKind::Git(ref git_ref) => {
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

                let cargo_home = home::cargo_home()?;
                let repo_path = cargo_home.join("git/db").join(&path);

                if repo_path.is_dir() {
                    let repo = Repository::open_bare(repo_path)?;
                    let mut remote = repo.remote_anonymous(package.source_id.url.as_str())?;

                    let target = match git_ref {
                        GitReference::Tag(_) => todo!("git tag"),
                        GitReference::Branch(b) => b,
                        GitReference::Rev(_) => continue, // don't move pinned revs
                        GitReference::DefaultBranch => "HEAD",
                    };

                    remote.fetch(&[target], None, None)?;

                    let head = match repo.find_reference(&format!("refs/remotes/origin/{target}")) {
                        Ok(r) => r.peel_to_commit()?,
                        Err(e) => bail!(
                            "HEAD error class: {:?}, code: {:?}, msg: {}",
                            e.class(),
                            e.code(),
                            e.message()
                        ),
                    };
                    let fetch_head = match repo.find_reference("FETCH_HEAD") {
                        Ok(r) => r.peel_to_commit()?,
                        Err(e) => bail!(
                            "FETCH_HEAD error class: {:?}, code: {:?}, msg: {}",
                            e.class(),
                            e.code(),
                            e.message()
                        ),
                    };

                    let changes = git_changes(&repo, &head, &fetch_head)?;

                    if changes.commits > 0 {
                        updates.git.insert(
                            package,
                            UpdateInfo::new(
                                info,
                                GitInfo {
                                    path,
                                    old_commit: head.id(),
                                    new_commit: fetch_head.id(),
                                    changes,
                                },
                            ),
                        );
                    }
                } else {
                    println!("    missing {}", path.red());
                }
            }
            SourceKind::Path => {
                if !args.path {
                    continue;
                }

                updates
                    .path
                    .insert(package, UpdateInfo::new(info, PathInfo {}));
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

                    updates.registry.insert(
                        package,
                        UpdateInfo::new(info, RegistryInfo { version: latest }),
                    );
                }
            }
        }
    }

    Ok(updates)
}

#[derive(Default)]
struct Updates {
    registry: BTreeMap<PackageId, UpdateInfo<RegistryInfo>>,
    git: BTreeMap<PackageId, UpdateInfo<GitInfo>>,
    path: BTreeMap<PackageId, UpdateInfo<PathInfo>>,
}

struct UpdateInfo<T> {
    install_info: InstallInfo,
    extra: T,
}

impl<T> UpdateInfo<T> {
    fn new(install_info: InstallInfo, extra: T) -> Self {
        Self {
            install_info,
            extra,
        }
    }
}

struct RegistryInfo {
    version: Version,
}

struct GitInfo {
    path: String,
    old_commit: Oid,
    new_commit: Oid,
    changes: GitChanges,
}

struct PathInfo {}

fn print_registry_updates(updates: &BTreeMap<PackageId, UpdateInfo<RegistryInfo>>) {
    if updates.is_empty() {
        println!("no {} crate updates", "registry".green());
    } else {
        let registry = updates
            .iter()
            .map(|(pkg, info)| (pkg.name.as_str(), &pkg.version, &info.extra.version))
            .collect::<RegistryTable>();

        println!("<<< Updates from the {} >>>", "registry".green());
        println!("\n{registry}\n");
    }
}

fn print_git_updates(updates: &BTreeMap<PackageId, UpdateInfo<GitInfo>>, enabled: bool) {
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
            .map(|(pkg, info)| (pkg.name.as_str(), &info.extra))
            .collect::<BTreeMap<_, _>>();

        println!("<<< Updates from {} >>>", "git".green());
        println!("\n{}\n", GitTable(gits));
    }
}

fn print_path_updates(updates: &BTreeMap<PackageId, UpdateInfo<PathInfo>>, enabled: bool) {
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
            .map(|(pkg, _)| pkg.name.as_str())
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

fn install_registry_updates(
    updates: impl ExactSizeIterator<Item = (PackageId, UpdateInfo<RegistryInfo>)>,
) {
    let count = updates.len();

    if count > 0 {
        println!(
            "start installing {} {} updates",
            count.blue().bold(),
            "registry".green().bold()
        );

        for (i, (pkg, info)) in updates.enumerate() {
            println!(
                "\n{} updating {} from {} to {}",
                format_args!("[{}/{}]", i + 1, count).bold(),
                pkg.name.green().bold(),
                pkg.version.blue().bold(),
                info.extra.version.blue().bold()
            );

            if let Err(e) =
                cargo_install_from_registry(&pkg.name, &info.extra.version, &info.install_info)
            {
                println!(
                    "installing {} {}:\n{e}",
                    pkg.name.green().bold(),
                    "failed".red().bold()
                )
            }
        }
    }
}

fn install_git_updates(updates: impl ExactSizeIterator<Item = (PackageId, UpdateInfo<GitInfo>)>) {
    let count = updates.len();

    if count > 0 {
        println!(
            "start installing {} {} updates",
            count.blue().bold(),
            "git".green().bold()
        );
    }
}

fn install_path_updates(updates: impl ExactSizeIterator<Item = (PackageId, UpdateInfo<PathInfo>)>) {
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

struct GitChanges {
    commits: usize,
    files_changed: usize,
    insertions: usize,
    deletions: usize,
}

fn git_changes<'r>(repo: &'r Repository, old: &Commit<'r>, new: &Commit<'r>) -> Result<GitChanges> {
    let (ahead, behind) = repo.graph_ahead_behind(old.id(), new.id())?;
    if ahead != 0 {
        println!("hm... local HEAD shouldn't be ahead of remote HEAD");
    }

    let diff_stats = repo
        .diff_tree_to_tree(Some(&old.tree()?), Some(&new.tree()?), None)?
        .stats()?;

    Ok(GitChanges {
        commits: behind,
        files_changed: diff_stats.files_changed(),
        insertions: diff_stats.insertions(),
        deletions: diff_stats.deletions(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_app() {
        use clap::CommandFactory;
        Opt::command().debug_assert();
    }

    #[test]
    fn parse_thingy() -> anyhow::Result<()> {
        let cargo_home = home::cargo_home()?;
        let repo = cargo_home.join("git/db/oxker-0e3309108aa09323");
        let repo = Repository::open_bare(repo)?;

        // Fetch the latest HEAD from remote.
        let mut remote = repo.remote_anonymous("https://github.com/mrjackwills/oxker.git")?;
        remote.fetch(&["HEAD"], None, None)?;

        // Get current HEAD and just fetched new FETCH_HEAD.
        let ref_old = repo
            .find_reference("refs/remotes/origin/HEAD")?
            .peel_to_commit()?;
        let ref_new = repo.find_reference("FETCH_HEAD")?.peel_to_commit()?;

        // Compare the two, counting amount of commits.
        let GitChanges {
            commits,
            files_changed,
            insertions,
            deletions,
        } = git_changes(&repo, &ref_old, &ref_new)?;

        println!(
            "{:.7} -> {:.7} ({} commits | {} files changed | {} {})",
            ref_old.id().cyan(),
            ref_new.id().cyan(),
            commits.yellow().bold(),
            files_changed.white(),
            format_args!("+{insertions}").green(),
            format_args!("-{deletions}").red(),
        );

        // Update HEAD to FETCH_HEAD.
        // r_old.set_target(r_new.id(), "")?;

        Ok(())
    }

    #[test]
    fn int_len() {
        let value = 150usize;
        let len = (value as f64).log10().floor() as usize + 1;

        assert_eq!(3, len);
    }
}
