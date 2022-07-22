//! Data structures used throughout the crate.

use std::collections::BTreeMap;

use git2::Oid;
use semver::Version;

use crate::cargo::{InstallInfo, PackageId};

#[derive(Default)]
pub struct Updates {
    pub registry: BTreeMap<PackageId, UpdateInfo<RegistryInfo>>,
    pub git: BTreeMap<PackageId, UpdateInfo<GitInfo>>,
    pub path: BTreeMap<PackageId, UpdateInfo<PathInfo>>,
}

pub struct UpdateInfo<T> {
    pub install_info: InstallInfo,
    pub extra: T,
}

impl<T> UpdateInfo<T> {
    pub fn new(install_info: InstallInfo, extra: T) -> Self {
        Self {
            install_info,
            extra,
        }
    }
}

pub struct RegistryInfo {
    pub version: Version,
}

pub struct GitInfo {
    pub r#type: String,
    pub old_commit: Oid,
    pub new_commit: Oid,
    pub changes: GitChanges,
}

pub struct GitChanges {
    pub commits: usize,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

pub struct PathInfo {}
