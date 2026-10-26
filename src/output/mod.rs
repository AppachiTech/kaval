//! Command-line output: human-readable tables and JSON.

pub mod json;
pub mod table;

use std::borrow::Cow;
use std::io::IsTerminal;

use clap::ValueEnum;

pub use table::print_check_table;

/// When to colour output (`--color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ColorChoice {
    /// Colour terminals only (honours NO_COLOR and CLICOLOR_FORCE)
    #[default]
    Auto,
    Always,
    Never,
}

/// Whether to emit colour for `choice` on a stream that is (or isn't) a terminal.
pub fn color_enabled(choice: ColorChoice, is_tty: bool) -> bool {
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let force = std::env::var("CLICOLOR_FORCE").is_ok_and(|v| !v.is_empty() && v != "0");
    color_enabled_with(choice, is_tty, no_color, force)
}

/// `NO_COLOR` beats `CLICOLOR_FORCE`; explicit `--color` beats both.
fn color_enabled_with(choice: ColorChoice, is_tty: bool, no_color: bool, force: bool) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => !no_color && (is_tty || force),
    }
}

/// Replace control characters (C0, DEL and C1) with `?`, so text supplied by
/// other processes (names, arguments, paths) cannot inject terminal escapes.
pub fn sanitize(s: &str) -> Cow<'_, str> {
    if s.chars().any(char::is_control) {
        Cow::Owned(
            s.chars()
                .map(|c| if c.is_control() { '?' } else { c })
                .collect(),
        )
    } else {
        Cow::Borrowed(s)
    }
}

/// Width of the terminal stdout is attached to, if it is one.
pub fn terminal_width() -> Option<u16> {
    if !std::io::stdout().is_terminal() {
        return None;
    }
    ratatui::crossterm::terminal::size().ok().map(|(w, _)| w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_controls() {
        assert_eq!(sanitize("a\x1b[2Jb"), "a?[2Jb");
        assert_eq!(sanitize("tab\there\r\n"), "tab?here??");
        assert_eq!(sanitize("c1\u{9b}31m"), "c1?31m", "C1 CSI");
        assert_eq!(sanitize("del\u{7f}"), "del?");
        assert!(matches!(
            sanitize("café 日本 🚀"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn color_choice_matrix() {
        // (choice, is_tty, NO_COLOR set, CLICOLOR_FORCE set) -> colour?
        let cases = [
            (ColorChoice::Always, false, true, false, true),
            (ColorChoice::Never, true, false, true, false),
            (ColorChoice::Auto, true, false, false, true),
            (ColorChoice::Auto, false, false, false, false),
            (ColorChoice::Auto, true, true, false, false),
            (ColorChoice::Auto, false, false, true, true),
            (ColorChoice::Auto, false, true, true, false),
        ];
        for (choice, tty, no_color, force, want) in cases {
            assert_eq!(
                color_enabled_with(choice, tty, no_color, force),
                want,
                "{choice:?} tty={tty} NO_COLOR={no_color} CLICOLOR_FORCE={force}"
            );
        }
    }
}
