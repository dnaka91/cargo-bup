use std::fmt::{self, Display};

use anstyle::{AnsiColor, Color, Effects, Style};

pub struct Styled<T> {
    display: T,
    style: Style,
}

impl<T> Styled<T> {
    pub const fn fg(value: T, color: AnsiColor) -> Self {
        Self {
            display: value,
            style: Style::new().fg_color(Some(Color::Ansi(color))),
        }
    }

    pub const fn effects(value: T, effects: Effects) -> Self {
        Self {
            display: value,
            style: Style::new().effects(effects),
        }
    }

    pub const fn bold(mut self) -> Self {
        self.style = self.style.bold();
        self
    }
}

impl<T: Display> Display for Styled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}",
            self.style.render(),
            self.display,
            self.style.render_reset()
        )
    }
}

pub fn green<T: Display>(value: T) -> Styled<T> {
    Styled::fg(value, AnsiColor::Green)
}

pub fn red<T: Display>(value: T) -> Styled<T> {
    Styled::fg(value, AnsiColor::Red)
}

pub fn yellow<T: Display>(value: T) -> Styled<T> {
    Styled::fg(value, AnsiColor::Yellow)
}

pub fn blue<T: Display>(value: T) -> Styled<T> {
    Styled::fg(value, AnsiColor::Blue)
}

pub fn cyan<T: Display>(value: T) -> Styled<T> {
    Styled::fg(value, AnsiColor::Cyan)
}

pub fn white<T: Display>(value: T) -> Styled<T> {
    Styled::fg(value, AnsiColor::White)
}

pub fn bold<T: Display>(value: T) -> Styled<T> {
    Styled::effects(value, Effects::BOLD)
}

pub fn dimmed<T: Display>(value: T) -> Styled<T> {
    Styled::effects(value, Effects::DIMMED)
}
