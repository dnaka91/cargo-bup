use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use cargo::{CanonicalUrl, InstallInfo, PackageId};
use clap::{Args, IntoApp, Parser, Subcommand};
use clap_complete::Shell;
use crates_index::Index;
use git2::{Commit, Oid, Repository, RepositoryInitOptions};
use owo_colors::OwoColorize;
use semver::Version;
use siphasher::sip::SipHasher24;

use crate::cargo::{CrateListingV2, GitReference, SourceKind};

mod cargo;
mod git;
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
    let index = load_index()?;
    let updates = collect_updates(info, &index, &cmd.select_args)?;

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

                let commit_id = match package.source_id.precise.as_deref() {
                    Some(c) => c.parse()?,
                    None => continue,
                };

                let repo_path = get_git_repo_path(&package.source_id.canonical_url)?;
                let repo = open_or_init_repo(&repo_path)?;

                let mut remote = repo.remote_anonymous(package.source_id.url.as_str())?;

                let (refspec, target, r#type) = match git_ref {
                    GitReference::Tag(_) => continue, // don't touch tags (yet)
                    GitReference::Branch(b) => (
                        format!("+refs/heads/{b}:refs/remotes/origin/{b}"),
                        format!("refs/remotes/origin/{b}"),
                        format!("branch {b}"),
                    ),
                    GitReference::Rev(_) => continue, // don't move pinned revs
                    GitReference::DefaultBranch => (
                        "+HEAD:refs/remotes/origin/HEAD".to_owned(),
                        "refs/remotes/origin/HEAD".to_owned(),
                        "HEAD".to_owned(),
                    ),
                };

                remote.fetch(&[refspec], None, None)?;

                let current = repo.find_commit(commit_id)?;
                let latest = repo.find_reference(&target)?.peel_to_commit()?;

                let changes = git_changes(&repo, &current, &latest)?;

                if changes.commits > 0 {
                    updates.git.insert(
                        package,
                        UpdateInfo::new(
                            info,
                            GitInfo {
                                r#type,
                                old_commit: current.id(),
                                new_commit: latest.id(),
                                changes,
                            },
                        ),
                    );
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
    r#type: String,
    old_commit: Oid,
    new_commit: Oid,
    changes: GitChanges,
}

struct PathInfo {}

struct ProgressGuard;

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        println!("done");
    }
}

fn progress(msg: fmt::Arguments) -> ProgressGuard {
    print!("{}... ", msg);
    std::io::stdout().flush().ok();
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

fn get_git_repo_path(canonical_url: &CanonicalUrl) -> Result<PathBuf> {
    let ident = canonical_url
        .0
        .path_segments()
        .and_then(|s| s.last())
        .unwrap_or("");

    let ident = if ident.is_empty() { "_empty" } else { ident };

    let mut hasher = SipHasher24::new();
    canonical_url.hash(&mut hasher);
    let hash = hex::encode(hasher.finish().to_le_bytes());
    let path = format!("{ident}-{hash}");

    let cargo_home = home::cargo_home()?;
    let repo_path = cargo_home.join("git/db").join(&path);

    Ok(repo_path)
}

fn open_or_init_repo(path: &Path) -> Result<Repository, git2::Error> {
    if path.is_dir() {
        Repository::open_bare(path)
    } else {
        Repository::init_opts(
            path,
            RepositoryInitOptions::new()
                .external_template(false)
                .bare(true),
        )
    }
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
