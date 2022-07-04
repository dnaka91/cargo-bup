use std::fmt::{self, Display};

use owo_colors::OwoColorize;
use semver::Version;

#[derive(Default)]
pub struct Table(Vec<[String; 3]>);

impl Table {
    pub fn add(&mut self, name: &str, current: &Version, latest: &Version) {
        self.0.push([
            name.to_owned(),
            current.to_string(),
            ColorizedVersion::new(current, latest).to_string(),
        ]);
    }
}

impl Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut column_widths = ["Name".len(), "Current".len()];

        for row in &self.0 {
            column_widths[0] = column_widths[0].max(row[0].len());
            column_widths[1] = column_widths[1].max(row[1].len());
        }

        writeln!(
            f,
            "\n  {} major · {} minor · {} patch\n",
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

impl<'a> FromIterator<(&'a str, &'a Version, &'a Version)> for Table {
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
