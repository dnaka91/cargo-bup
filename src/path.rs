//! Handling of crates that were installed from **local paths**.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use owo_colors::OwoColorize;

use crate::{
    cargo::PackageId,
    models::{PathInfo, UpdateInfo},
};

pub(crate) fn check_update(_package: &PackageId, _path: bool) -> Result<Option<PathInfo>> {
    if !_path {
        return Ok(None);
    }

    Ok(Some(PathInfo {}))
}

pub(crate) fn print_updates(updates: &BTreeMap<PackageId, UpdateInfo<PathInfo>>, enabled: bool) {
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

pub(crate) fn install_updates(
    updates: impl ExactSizeIterator<Item = (PackageId, UpdateInfo<PathInfo>)>,
    _quiet: bool,
) {
    let count = updates.len();

    if count > 0 {
        println!(
            "start installing {} {} updates",
            count.blue().bold(),
            "local path".green().bold()
        );
    }
}
