//! Handling of crates that were installed from **Git repositories**.

use std::collections::BTreeMap;

use owo_colors::OwoColorize;

use crate::{cargo::PackageId, table::GitTable, GitInfo, UpdateInfo};

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
        let gits = updates
            .iter()
            .map(|(pkg, info)| (pkg.name.as_str(), &info.extra))
            .collect::<BTreeMap<_, _>>();

        println!("<<< Updates from {} >>>", "git".green());
        println!("\n{}\n", GitTable(gits));
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
