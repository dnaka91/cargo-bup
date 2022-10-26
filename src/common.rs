use std::process::{Command, Stdio};

use anyhow::Result;

use crate::cargo::InstallInfo;

pub fn apply_cmd_args(cmd: &mut Command, info: &InstallInfo) {
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
}

pub fn run_cmd(mut cmd: Command, name: &str, quiet: bool) -> Result<()> {
    if !quiet {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }

    let output = cmd.output()?;

    if !output.status.success() {
        if quiet {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("\nfailed installing `{name}`:\n{stderr}")
        } else {
            eprintln!("\nfailed installing `{name}`");
        }
    }

    Ok(())
}
