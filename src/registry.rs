//! Handling of crates that were installed from **the main <https://crates.io> registry**.

use std::{collections::BTreeMap, process::Command};

use anyhow::{Context, Result};
use crates_index::Index;
use owo_colors::OwoColorize;
use semver::Version;

use crate::{
    cargo::{InstallInfo, PackageId},
    models::{RegistryInfo, UpdateInfo},
    table::RegistryTable,
};

/// Remote Git repository location for the main <https://crates.io> registry.
const CRATES_IO_GIT_URL: &str = "https://github.com/rust-lang/crates.io-index";

pub(crate) fn check_update(
    index: &Index,
    package: &PackageId,
    pre: bool,
) -> Result<Option<RegistryInfo>> {
    if package.source_id.url.as_str() != CRATES_IO_GIT_URL {
        // Currently only support the main crates.io registry.
        return Ok(None);
    }

    let krate = index
        .crate_(&package.name)
        .context("failed finding package")?;

    let latest = Version::parse(krate.latest_version().version())?;

    if !latest.pre.is_empty() && !pre {
        return Ok(None);
    }

    Ok((latest > package.version).then(|| RegistryInfo { version: latest }))
}

pub(crate) fn print_updates(updates: &BTreeMap<PackageId, UpdateInfo<RegistryInfo>>) {
    if updates.is_empty() {
        println!("no {} crate updates", "registry".green());
    } else {
        let table = updates
            .iter()
            .map(|(pkg, info)| (pkg.name.as_str(), &pkg.version, &info.extra.version))
            .collect::<RegistryTable>();

        println!("<<< Updates from the {} >>>", "registry".green());
        println!("\n{table}\n");
    }
}

pub(crate) fn install_updates(
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

            if let Err(e) = cargo_install(&pkg.name, &info.extra.version, &info.install_info) {
                println!(
                    "installing {} {}:\n{e}",
                    pkg.name.green().bold(),
                    "failed".red().bold()
                )
            }
        }
    }
}

fn cargo_install(name: &str, version: &Version, info: &InstallInfo) -> Result<()> {
    let mut cmd = Command::new("cargo");
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
