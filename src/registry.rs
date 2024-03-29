//! Handling of crates that were installed from **the main <https://crates.io> registry**.

use std::{collections::BTreeMap, process::Command};

use anstream::{eprintln, println};
use anyhow::{Context, Result};
use crates_index::GitIndex;
use semver::Version;

use crate::{
    cargo::{InstallInfo, PackageId},
    colors, common,
    models::{RegistryInfo, UpdateInfo},
    table::RegistryTable,
};

/// Remote Git repository location for the main <https://crates.io> registry.
const CRATES_IO_GIT_URL: &str = "https://github.com/rust-lang/crates.io-index";

pub(crate) fn check_update(
    index: &GitIndex,
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

    let latest = Version::parse(krate.most_recent_version().version())?;

    if !latest.pre.is_empty() && !pre {
        return Ok(None);
    }

    Ok((latest > package.version).then_some(RegistryInfo { version: latest }))
}

pub(crate) fn print_updates(updates: &BTreeMap<PackageId, UpdateInfo<RegistryInfo>>) {
    if updates.is_empty() {
        println!("no {} crate updates", colors::green("registry"));
    } else {
        let table = updates
            .iter()
            .map(|(pkg, info)| (pkg.name.as_str(), &pkg.version, &info.extra.version))
            .collect::<RegistryTable>();

        println!("\n{table}\n");
    }
}

pub(crate) fn install_updates(
    updates: impl ExactSizeIterator<Item = (PackageId, UpdateInfo<RegistryInfo>)>,
    quiet: bool,
) {
    let count = updates.len();
    if count == 0 {
        return;
    }

    println!(
        "start installing {} {} updates\n",
        colors::blue(count).bold(),
        colors::green("registry").bold()
    );

    for (i, (pkg, info)) in updates.enumerate() {
        println!(
            "{} updating {} from {} to {}",
            colors::bold(format_args!("[{}/{}]", i + 1, count)),
            colors::green(&pkg.name).bold(),
            colors::blue(pkg.version).bold(),
            colors::blue(&info.extra.version).bold()
        );

        if let Err(e) = cargo_install(&pkg.name, &info.extra.version, &info.install_info, quiet) {
            eprintln!(
                "\ninstalling {} {}:\n{e}",
                colors::green(pkg.name).bold(),
                colors::red("failed").bold()
            )
        }
    }
}

fn cargo_install(name: &str, version: &Version, info: &InstallInfo, quiet: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args(["install", name]);

    cmd.arg("--version");
    cmd.arg(version.to_string());

    common::apply_cmd_args(&mut cmd, info);
    common::run_cmd(cmd, name, quiet)
}
