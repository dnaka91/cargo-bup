//! Handling of crates that were installed from **Git repositories**.

use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use gix::{remote::Direction, Commit, ObjectId, Repository};
use owo_colors::OwoColorize;
use siphasher::sip::SipHasher24;

use crate::{
    cargo::{CanonicalUrl, GitReference, InstallInfo, PackageId},
    common,
    models::{GitChanges, GitInfo, GitTarget, UpdateInfo},
    table::GitTable,
};

pub(crate) fn check_update(
    package: &PackageId,
    git_ref: &GitReference,
    git: bool,
) -> Result<Option<GitInfo>> {
    if !git {
        return Ok(None);
    }

    let commit_id = match package.source_id.precise.as_deref() {
        Some(c) => c.parse::<ObjectId>()?,
        None => return Ok(None),
    };

    let repo_path = get_git_repo_path(&package.source_id.canonical_url)?;
    let repo = open_or_init_repo(&repo_path)?;

    let mut remote = repo.remote_at(package.source_id.url.as_str())?;

    let (refspec, target, r#type, git_target) = match git_ref {
        GitReference::Tag(_) => return Ok(None), // don't touch tags (yet)
        GitReference::Branch(b) => (
            format!("+refs/heads/{b}:refs/remotes/origin/{b}"),
            format!("refs/remotes/origin/{b}"),
            format!("branch {b}"),
            GitTarget::Branch(b.clone()),
        ),
        GitReference::Rev(_) => return Ok(None), // don't move pinned revs
        GitReference::DefaultBranch => (
            "+HEAD:refs/remotes/origin/HEAD".to_owned(),
            "refs/remotes/origin/HEAD".to_owned(),
            "HEAD".to_owned(),
            GitTarget::Default,
        ),
    };

    remote.replace_refspecs([refspec.as_str()], Direction::Fetch)?;
    remote
        .connect(Direction::Fetch)?
        .prepare_fetch(gix::progress::Discard, Default::default())?
        .receive(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)?;

    let current = repo.find_object(commit_id)?.try_into_commit()?;
    let latest = repo
        .find_reference(&target)?
        .into_fully_peeled_id()?
        .object()?
        .try_into_commit()?;

    let changes = git_changes(&repo, &current, &latest)?;

    Ok((changes.commits > 0).then_some(GitInfo {
        r#type,
        old_commit: current.id,
        new_commit: latest.id,
        changes,
        target: git_target,
    }))
}

pub(crate) fn print_updates(updates: &BTreeMap<PackageId, UpdateInfo<GitInfo>>, enabled: bool) {
    if !enabled {
        println!(
            "{} crate updates {}",
            "git".green(),
            "disabled".yellow().bold()
        );
    } else if updates.is_empty() {
        println!("no {} crate updates", "git".green());
    } else {
        let table = updates
            .iter()
            .map(|(pkg, info)| (pkg.name.as_str(), &info.extra))
            .collect::<GitTable>();

        println!("\n{table}\n");
    }
}

pub(crate) fn install_updates(
    updates: impl ExactSizeIterator<Item = (PackageId, UpdateInfo<GitInfo>)>,
    quiet: bool,
) {
    let count = updates.len();
    if count == 0 {
        return;
    }

    println!(
        "start installing {} {} updates\n",
        count.blue().bold(),
        "git".green().bold()
    );

    for (i, (pkg, info)) in updates.enumerate() {
        println!(
            "{} updating {} from {} to {}",
            format_args!("[{}/{}]", i + 1, count).bold(),
            pkg.name.green().bold(),
            info.extra.old_commit.blue().bold(),
            info.extra.new_commit.blue().bold()
        );

        if let Err(e) = cargo_install(
            &pkg.name,
            pkg.source_id.url.as_str(),
            &info.extra.target,
            &info.install_info,
            quiet,
        ) {
            println!(
                "\ninstalling {} {}:\n{e}",
                pkg.name.green().bold(),
                "failed".red().bold()
            )
        }
    }
}

fn cargo_install(
    name: &str,
    git_url: &str,
    git_ref: &GitTarget,
    info: &InstallInfo,
    quiet: bool,
) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args(["install", name]);
    cmd.args(["--git", git_url]);

    match git_ref {
        GitTarget::Default => {} // This is the default, so nothing to do
        GitTarget::Branch(b) => {
            cmd.args(["--branch", b]);
        }
    }

    common::apply_cmd_args(&mut cmd, info);
    common::run_cmd(cmd, name, quiet)
}

fn git_changes<'r>(repo: &'r Repository, old: &Commit<'r>, new: &Commit<'r>) -> Result<GitChanges> {
    use gix::{
        diff::blob::pipeline::{Mode, WorktreeRoots},
        object::tree::diff::Action,
    };

    let commits = new
        .ancestors()
        .all()?
        .map_while(Result::ok)
        .take_while(|commit| commit.id != new.id)
        .count();

    if commits == 0 {
        return Ok(GitChanges::default());
    }

    let mut diff_cache = repo
        .diff_resource_cache(
            Mode::default(),
            WorktreeRoots {
                old_root: None,
                new_root: None,
            },
        )
        .context("diff resource")?;
    let mut files_changed = 0;
    let mut insertions = 0;
    let mut deletions = 0;

    old.tree()
        .context("tree")?
        .changes()
        .context("changes")?
        .for_each_to_obtain_tree(&new.tree()?, |change| {
            files_changed += 1;

            if let Some(counts) = change.diff(&mut diff_cache)?.line_counts()? {
                insertions += counts.insertions as usize;
                deletions += counts.removals as usize;
            }

            anyhow::Ok(Action::Continue)
        })
        .context("for each")?;

    Ok(GitChanges {
        commits,
        files_changed,
        insertions,
        deletions,
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
    let repo_path = cargo_home.join("git/db").join(path);

    Ok(repo_path)
}

fn open_or_init_repo(path: &Path) -> Result<Repository> {
    if path.is_dir() {
        gix::open_opts(path, gix::open::Options::isolated()).map_err(Into::into)
    } else {
        gix::init_bare(path).map_err(Into::into)
    }
}
