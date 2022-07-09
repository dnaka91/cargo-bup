use std::{
    collections::BTreeMap,
    fmt::{self, Display},
};

use owo_colors::OwoColorize;
use semver::Version;

use crate::GitInfo;

#[derive(Default)]
pub struct RegistryTable(Vec<[String; 3]>);

impl RegistryTable {
    pub fn add(&mut self, name: &str, current: &Version, latest: &Version) {
        self.0.push([
            name.to_owned(),
            current.to_string(),
            ColorizedVersion::new(current, latest).to_string(),
        ]);
    }
}

impl Display for RegistryTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut column_widths = ["Name".len(), "Current".len()];

        for row in &self.0 {
            column_widths[0] = column_widths[0].max(row[0].len());
            column_widths[1] = column_widths[1].max(row[1].len());
        }

        writeln!(
            f,
            "  {} major · {} minor · {} patch\n",
            "◆".yellow(),
            "◆".green(),
            "◆".blue()
        )?;

        write!(f, "{:width$}", "Name", width = column_widths[0] + 2)?;
        write!(f, "{:width$}", "Current", width = column_widths[1] + 5)?;
        writeln!(f, "Latest")?;
        writeln!(
            f,
            "{}",
            "─".repeat(column_widths[0] + column_widths[1] + 16)
        )?;

        for row in &self.0 {
            write!(f, "{:width$}", row[0], width = column_widths[0] + 2)?;
            write!(f, "{:width$}", row[1], width = column_widths[1] + 2)?;
            writeln!(f, "➞  {}", row[2])?;
        }

        Ok(())
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

struct ColorizedVersion<'a> {
    current: &'a Version,
    latest: &'a Version,
}

impl<'a> ColorizedVersion<'a> {
    fn new(current: &'a Version, latest: &'a Version) -> Self {
        Self { current, latest }
    }
}

impl<'a> Display for ColorizedVersion<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let major = self.latest.major;
        let minor = self.latest.minor;
        let patch = self.latest.patch;

        if self.current.major != self.latest.major {
            write!(f, "{}", format_args!("{major}.{minor}.{patch}").yellow())?;
        } else if self.current.minor != self.latest.minor {
            write!(f, "{major}.{}", format_args!("{minor}.{patch}").green())?;
        } else {
            write!(f, "{major}.{minor}.{}", patch.blue())?;
        }

        if !self.latest.pre.is_empty() {
            write!(f, "-{}", self.latest.pre.dimmed())?;
        }

        if !self.latest.build.is_empty() {
            write!(f, "+{}", self.latest.build.dimmed())?;
        }

        Ok(())
    }
}

pub struct GitTable<'a>(pub(crate) BTreeMap<&'a str, &'a GitInfo>);

impl<'a> Display for GitTable<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self
            .0
            .keys()
            .max_by_key(|k| k.len())
            .map(|k| k.len())
            .unwrap_or(4);

        writeln!(f, "{:width$}  Old         New      Changes", "Name")?;
        writeln!(f, "{}", "─".repeat(width * 2 + 2 + 1 + 16 + 20))?;

        for (name, info) in &self.0 {
            writeln!(
                f,
                "{name:width$}  {:.7}  ➞  {:.7}  {:2} commits | {:2} files changed | {} {}",
                info.old_commit.cyan(),
                info.new_commit.cyan(),
                info.changes.commits.yellow(),
                info.changes.files_changed.white(),
                format_args!("+{}", info.changes.insertions).green(),
                format_args!("-{}", info.changes.deletions).red()
            )?;
        }

        Ok(())
    }
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
