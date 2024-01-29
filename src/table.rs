//! Printing of data (to the terminal) in a table-ish formatting.

use std::fmt::{self, Display};

use anstyle::AnsiColor;
use gix::ObjectId;
use semver::Version;
use tabled::{
    settings::{
        object::{Columns, Rows, Segment},
        style::{Border, HorizontalLine, Style},
        Alignment, Modify, Padding, Panel,
    },
    Table, Tabled,
};

use crate::{colors, models::GitInfo};

/// The registry table prints updates for crates that come directly from the a crate registry.
#[derive(Default)]
pub struct RegistryTable(Vec<RegistryRow>);

impl RegistryTable {
    pub fn add(&mut self, name: &str, current: &Version, latest: &Version) {
        self.0.push(RegistryRow {
            name: name.to_owned(),
            current: current.to_string(),
            latest: ColorizedVersion::new(current, latest).to_string(),
        });
    }
}

impl<'a> FromIterator<(&'a str, &'a Version, &'a Version)> for RegistryTable {
    fn from_iter<T: IntoIterator<Item = (&'a str, &'a Version, &'a Version)>>(iter: T) -> Self {
        let mut table = Self::default();
        for (name, current, latest) in iter {
            table.add(name, current, latest);
        }

        table
    }
}

impl Display for RegistryTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{}",
            Table::new(&self.0)
                // Add color legend as header
                .with(Panel::header(format!(
                    "{} major · {} minor · {} patch",
                    colors::yellow("◆"),
                    colors::green("◆"),
                    colors::blue("◆")
                )))
                .with(Panel::header(
                    colors::green(format_args!("Updates from the {}", "registry"))
                        .bold()
                        .to_string()
                ))
                // Align headers to the center
                .with(
                    Modify::new(Rows::new(0..=1))
                        .with(Alignment::center())
                        .with(Padding::new(1, 1, 0, 1))
                )
                // Draw straight line under the headers
                .with(Style::blank().horizontals([(3, HorizontalLine::new('─').intersection('─'))]))
                // Draw arrow between current and latest version
                .with(Modify::new(Segment::new(3.., 1..=1)).with(Border::new().set_right('➞')))
                // Add spacing between current and latest version
                .with(Modify::new(Columns::single(1)).with(Padding::new(1, 2, 0, 0)))
                .with(Modify::new(Columns::single(2)).with(Padding::new(2, 1, 0, 0)))
        )
    }
}

/// Single row for the [`RegistryTable`], that can be used with [`tabled`].
#[derive(Tabled)]
#[tabled(rename_all = "PascalCase")]
struct RegistryRow {
    name: String,
    current: String,
    latest: String,
}

/// A SemVer version that is a colored, based on how much two versions differ from one another. The
/// higher the difference, the stronger colors are used.
struct ColorizedVersion<'a> {
    current: &'a Version,
    latest: &'a Version,
}

impl<'a> ColorizedVersion<'a> {
    fn new(current: &'a Version, latest: &'a Version) -> Self {
        Self { current, latest }
    }

    fn select_colors(&self) -> [AnsiColor; 3] {
        let major = (self.current.major, self.latest.major);
        let minor = (self.current.minor, self.latest.minor);

        match (major, minor) {
            ((0, 0), (0, 0)) => [AnsiColor::Yellow; 3],
            ((0, 0), _) => [AnsiColor::Yellow, AnsiColor::Yellow, AnsiColor::Green],
            _ => [AnsiColor::Yellow, AnsiColor::Green, AnsiColor::Blue],
        }
    }
}

impl<'a> Display for ColorizedVersion<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let major = self.latest.major;
        let minor = self.latest.minor;
        let patch = self.latest.patch;

        let colors = self.select_colors();

        if self.current.major != self.latest.major {
            write!(
                f,
                "{}",
                colors::Styled::fg(format_args!("{major}.{minor}.{patch}"), colors[0])
            )?;
        } else if self.current.minor != self.latest.minor {
            write!(
                f,
                "{major}.{}",
                colors::Styled::fg(format_args!("{minor}.{patch}"), colors[1])
            )?;
        } else {
            write!(
                f,
                "{major}.{minor}.{}",
                colors::Styled::fg(patch, colors[2])
            )?;
        }

        if !self.latest.pre.is_empty() {
            write!(f, "-{}", colors::dimmed(&self.latest.pre))?;
        }

        if !self.latest.build.is_empty() {
            write!(f, "+{}", colors::dimmed(&self.latest.build))?;
        }

        Ok(())
    }
}

/// The git table prints updates for crates that come from remote Git repositories.
#[derive(Default)]
pub struct GitTable<'a>(Vec<GitRow<'a>>);

impl<'a> GitTable<'a> {
    pub fn add(&mut self, name: &'a str, info: &'a GitInfo) {
        self.0.push(GitRow {
            name,
            r#type: &info.r#type,
            old_commit: info.old_commit,
            new_commit: info.new_commit,
            commits: info.changes.commits,
            files_changed: info.changes.files_changed,
            insertions: info.changes.insertions,
            deletions: info.changes.deletions,
        });
    }
}

impl<'a> FromIterator<(&'a str, &'a GitInfo)> for GitTable<'a> {
    fn from_iter<T: IntoIterator<Item = (&'a str, &'a GitInfo)>>(iter: T) -> Self {
        let mut table = Self::default();
        for (name, info) in iter {
            table.add(name, info);
        }

        table
    }
}

impl<'a> Display for GitTable<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{}",
            Table::new(&self.0)
                .with(Panel::header(
                    colors::green(format_args!("Updates from {}", "git"))
                        .bold()
                        .to_string()
                ))
                .with(
                    Modify::new(Rows::first())
                        .with(Alignment::center())
                        .with(Padding::new(1, 1, 0, 1))
                )
                // Draw strait line under the headers
                .with(Style::blank().horizontals([(2, HorizontalLine::new('─').intersection('─'))]))
                // Draw arrow between old and new commit
                .with(Modify::new(Segment::new(2.., 2..=2)).with(Border::new().set_right('➞')),)
                // Add spacing between old and new commit
                .with(Modify::new(Columns::single(2)).with(Padding::new(1, 2, 0, 0)))
                .with(Modify::new(Columns::single(3)).with(Padding::new(2, 1, 0, 0)))
                // Align commit details and reduce padding
                .with(
                    Modify::new(Segment::new(2.., 4..))
                        .with(Alignment::right())
                        .with(Padding::zero()),
                )
                .with(Modify::new(Columns::single(4)).with(Padding::new(1, 0, 0, 0)))
                .with(Modify::new(Columns::single(7)).with(Padding::new(0, 1, 0, 0)))
        )
    }
}

/// Single row for the [`GitTable`], that can be used with [`tabled`].
#[derive(Tabled)]
struct GitRow<'a> {
    #[tabled(rename = "Name")]
    name: &'a str,
    #[tabled(rename = "Type", display_with = "display_type")]
    r#type: &'a str,
    #[tabled(rename = "Old", display_with = "display_commit")]
    old_commit: ObjectId,
    #[tabled(rename = "New", display_with = "display_commit")]
    new_commit: ObjectId,
    #[tabled(rename = "Changes", display_with = "display_commit_count")]
    commits: usize,
    #[tabled(rename = "", display_with = "display_files_changed")]
    files_changed: usize,
    #[tabled(rename = "", display_with = "display_insertions")]
    insertions: usize,
    #[tabled(rename = "", display_with = "display_deletions")]
    deletions: usize,
}

fn display_type(value: &str) -> String {
    colors::blue(value).to_string()
}

fn display_commit(value: &ObjectId) -> String {
    format!("{:.7}", colors::cyan(value))
}

fn display_commit_count(value: &usize) -> String {
    format!("{} commits", colors::yellow(value))
}

fn display_files_changed(value: &usize) -> String {
    format!("{} files changed", colors::white(value))
}

fn display_insertions(value: &usize) -> String {
    colors::green(format_args!("+{value}")).to_string()
}

fn display_deletions(value: &usize) -> String {
    colors::red(format_args!("-{value}")).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorized_versions() {
        // 1.0.1
        //     ^-- patch only
        assert_eq!(
            "1.0.\x1b[34m1\x1b[39m",
            ColorizedVersion::new(&Version::new(1, 0, 0), &Version::new(1, 0, 1)).to_string()
        );
        // 1.1.0
        //   ^^^-- minor + patch
        assert_eq!(
            "1.\x1b[32m1.0\x1b[39m",
            ColorizedVersion::new(&Version::new(1, 0, 0), &Version::new(1, 1, 0)).to_string()
        );
        // 2.0.0
        // ^^^^^-- major + minor + patch
        assert_eq!(
            "\x1b[33m2.0.0\x1b[39m",
            ColorizedVersion::new(&Version::new(1, 0, 0), &Version::new(2, 0, 0)).to_string()
        );
        // 2.0.0-abc+1
        // ^^^^^ ^^^ ^-- major + minor + path, then pre, then build,
        //               but not the pre/build separators
        assert_eq!(
            "\x1b[33m2.0.0\x1b[39m-\x1b[2mabc\x1b[0m+\x1b[2m1\x1b[0m",
            ColorizedVersion::new(
                &Version::new(1, 0, 0),
                &Version::parse("2.0.0-abc+1").unwrap()
            )
            .to_string()
        );
    }
}
