//! Handling of crates that were installed from **Git repositories**.

use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use anyhow::Result;
use git2::{Commit, Repository, RepositoryInitOptions};
use owo_colors::OwoColorize;
use siphasher::sip::SipHasher24;

use crate::{
    cargo::{CanonicalUrl, GitReference, PackageId},
    models::{GitChanges, GitInfo, UpdateInfo},
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
        Some(c) => c.parse()?,
        None => return Ok(None),
    };

    let repo_path = get_git_repo_path(&package.source_id.canonical_url)?;
    let repo = open_or_init_repo(&repo_path)?;

    let mut remote = repo.remote_anonymous(package.source_id.url.as_str())?;

    let (refspec, target, r#type) = match git_ref {
        GitReference::Tag(_) => return Ok(None), // don't touch tags (yet)
        GitReference::Branch(b) => (
            format!("+refs/heads/{b}:refs/remotes/origin/{b}"),
            format!("refs/remotes/origin/{b}"),
            format!("branch {b}"),
        ),
        GitReference::Rev(_) => return Ok(None), // don't move pinned revs
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

    Ok((changes.commits > 0).then(|| GitInfo {
        r#type,
        old_commit: current.id(),
        new_commit: latest.id(),
        changes,
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

        println!("<<< Updates from {} >>>", "git".green());
        println!("\n{table}\n");
    }
}

pub(crate) fn install_updates(
    updates: impl ExactSizeIterator<Item = (PackageId, UpdateInfo<GitInfo>)>,
) {
    let count = updates.len();

    if count > 0 {
        println!(
            "start installing {} {} updates",
            count.blue().bold(),
            "git".green().bold()
        );
    }
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
