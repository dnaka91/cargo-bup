use std::{fmt, fs::File, io::Write, sync::Arc};

use anyhow::Result;
use clap::{Args, IntoApp, Parser, Subcommand};
use clap_complete::Shell;
use crates_index::Index;
use owo_colors::OwoColorize;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use thread_local::ThreadLocal;

use crate::{
    cargo::{CrateListingV2, SourceKind},
    models::{UpdateInfo, Updates},
};

mod cargo;
mod git;
mod models;
mod path;
mod registry;
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
    update_index()?;
    let updates = collect_updates(info, &cmd.select_args)?;

    println!();

    registry::print_updates(&updates.registry);
    git::print_updates(&updates.git, cmd.select_args.git);
    path::print_updates(&updates.path, cmd.select_args.path);

    println!();

    if !cmd.dry_run {
        registry::install_updates(updates.registry.into_iter());
        git::install_updates(updates.git.into_iter());
        path::install_updates(updates.path.into_iter());
    }

    Ok(())
}

/// Get the current state of all installed crates from the `.crates2.json` file that cargo
/// maintains for all binaries.
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

/// Load and udpate the crates.io registry to the latest version from remote.
fn update_index() -> Result<()> {
    let _guard = progress(format_args!(
        "{} updating {}",
        "[2/3]".bold(),
        "crates.io index".green().bold()
    ));

    let mut index = Index::new_cargo_default()?;
    index.update()?;

    Ok(())
}

fn collect_updates(info: CrateListingV2, args: &SelectArgs) -> Result<Updates> {
    let _guard = progress(format_args!(
        "{} collecting {}",
        "[3/3]".bold(),
        "updates".green().bold()
    ));

    let tls = Arc::new(ThreadLocal::new());

    info.installs
        .into_par_iter()
        .try_fold(Updates::default, |mut updates, (package, info)| {
            match package.source_id.kind {
                SourceKind::Git(ref git_ref) => {
                    if let Some(update) = git::check_update(&package, git_ref, args.git)? {
                        updates.git.insert(package, UpdateInfo::new(info, update));
                    }
                }
                SourceKind::Path => {
                    if let Some(update) = path::check_update(&package, args.path)? {
                        updates.path.insert(package, UpdateInfo::new(info, update));
                    }
                }
                SourceKind::Registry => {
                    let tls = Arc::clone(&tls);
                    let index = tls.get_or_try(Index::new_cargo_default)?;

                    if let Some(update) = registry::check_update(index, &package, args.pre)? {
                        updates
                            .registry
                            .insert(package, UpdateInfo::new(info, update));
                    }
                }
            }
            anyhow::Ok(updates)
        })
        .try_reduce(Updates::default, |mut a, mut b| {
            a.registry.append(&mut b.registry);
            a.git.append(&mut b.git);
            a.path.append(&mut b.path);
            Ok(a)
        })
}

/// A guard created by [`progress`], that will finish the currently written line with a `done`
/// at the end, when this guard is dropped.
struct ProgressGuard;

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        println!("done");
    }
}

/// Print the given message, without writing a new line. Instead a [`ProgressGuard`] is returned,
/// that will finish the current line when dropped.
fn progress(msg: fmt::Arguments) -> ProgressGuard {
    print!("{msg}... ");
    std::io::stdout().flush().ok();
    ProgressGuard
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
    fn int_len() {
        let value = 150usize;
        let len = (value as f64).log10().floor() as usize + 1;

        assert_eq!(3, len);
    }
}
