//! Data structures used throughout the crate.

use std::collections::BTreeMap;

use gix::ObjectId;
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
    pub old_commit: ObjectId,
    pub new_commit: ObjectId,
    pub changes: GitChanges,
    pub target: GitTarget,
}

#[derive(Default)]
pub struct GitChanges {
    pub commits: usize,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

pub enum GitTarget {
    Default,
    Branch(String),
}

pub struct PathInfo {}
