use std::{fmt, fs::File, io::Write, sync::Arc};

use anyhow::Result;
use cli::SelectArgs;
use crates_index::GitIndex;
use owo_colors::OwoColorize;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use thread_local::ThreadLocal;

use crate::{
    cargo::{CrateListingV2, SourceKind},
    cli::Subcmd,
    models::{UpdateInfo, Updates},
};

mod cargo;
mod cli;
mod common;
mod git;
mod models;
mod path;
mod registry;
mod table;

fn main() -> Result<()> {
    let cmd = cli::parse();

    if let Some(Subcmd::Completions { shell }) = cmd.subcmd {
        cli::completions(shell);
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
        registry::install_updates(updates.registry.into_iter(), cmd.quiet);
        git::install_updates(updates.git.into_iter(), cmd.quiet);
        path::install_updates(updates.path.into_iter(), cmd.quiet);
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

/// Load and update the crates.io registry to the latest version from remote.
fn update_index() -> Result<()> {
    let _guard = progress(format_args!(
        "{} updating {}",
        "[2/3]".bold(),
        "crates.io index".green().bold()
    ));

    let mut index = GitIndex::new_cargo_default()?;
    index.update()?;

    Ok(())
}

/// Fetch updates for all installed binaries, eventually filtering out entries, based on the user
/// provided filter flags (or rather including flags).
///
/// The update information is collected into several lists, one for each source, as the printable
/// information and installation logic varies for each source.
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
                    let index = tls.get_or_try(GitIndex::new_cargo_default)?;

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
