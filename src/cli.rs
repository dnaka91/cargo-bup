//! Command line interface related logic.

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cli {
    /// Update your cargo-installed binaries.
    Bup(Command),
}

/// Update your cargo-installed binaries.
#[derive(Parser)]
#[command(about, author, version)]
pub struct Command {
    /// Arguments focused around selecting different kind of updates.
    #[command(flatten)]
    pub select_args: SelectArgs,
    /// Do an update check, but don't start any actual update installations.
    #[arg(short = 'n', long)]
    pub dry_run: bool,
    /// Optional sub-commands that can be triggered.
    #[command(subcommand)]
    pub subcmd: Option<Subcmd>,
}

/// Arguments for selecting categories of updates, mostly based on the type of crate sources that
/// are allowed, as some of these might take extra time to check.
#[derive(Args)]
pub struct SelectArgs {
    /// Include pre-releases in updates.
    #[arg(long)]
    pub pre: bool,
    /// Include crates installed from git repos (potentially slow).
    ///
    /// To find updates, each crate's local Git repository is updated against the remote repo.
    #[arg(long)]
    pub git: bool,
    /// Include crates installed from local paths (potentially slow).
    ///
    /// There is no way of checking the freshness for a crate that was installed locally, so cargo
    /// is invoked for each entry unconditionally.
    #[arg(long)]
    pub path: bool,
}

/// Any sub-commands that are trigger extra behavior, not part of the main function of this plugin.
#[derive(Subcommand)]
pub enum Subcmd {
    /// Generate shell completions, writing them to the standard output.
    Completions {
        /// The shell type to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// Parse the CLI arguments, returning them as well structured data.
///
/// Cargo plugins need to be wrapped in an extra structure and _pretend_ they are cargo, as they're
/// call in the form of `cargo-<plugin> <plugin> args...`. This function takes care of that and
/// hides all the wrapping data structures.
#[must_use]
pub fn parse() -> Command {
    let Cli::Bup(cmd) = Cli::parse();
    cmd
}

/// Generate auto-completions scripts for the given shell and print them to **stdout**. The user
/// is responsible to save these completions at the right location for the shell.
pub fn completions(shell: Shell) {
    clap_complete::generate(
        shell,
        &mut Cli::command(),
        env!("CARGO_PKG_NAME"),
        &mut std::io::stdout().lock(),
    );
}

#[cfg(test)]
mod tests {
    use super::Cli;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
