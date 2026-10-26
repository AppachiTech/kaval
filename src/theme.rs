//! Colour themes: dark, light and mono (NO_COLOR), with a 256-colour fallback
//! for terminals without true colour.

use std::io::IsTerminal;
use std::sync::OnceLock;
use std::time::Duration;

use clap::ValueEnum;
use ratatui::crossterm::style::Color as CtColor;
use ratatui::style::Color;

use crate::models::{Protocol, ServiceCategory};

/// `--theme` / `KAV_THEME`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ThemeChoice {
    /// Match the terminal background (COLORFGBG, then asking the terminal)
    #[default]
    Auto,
    Dark,
    Light,
}

/// The palette in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeName {
    Dark,
    Light,
    /// No colours at all (NO_COLOR / --color never): terminal defaults plus bold/reverse.
    Mono,
}

/// How many colours the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    Ansi256,
}

pub struct Theme {
    pub primary: Color,
    pub border: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub dev_server: Color,
    pub database: Color,
    pub cache: Color,
    pub queue: Color,
    pub container: Color,
    pub browser: Color,
    pub system: Color,
    /// No colours: highlight with reverse video instead.
    pub mono: bool,
}

impl Theme {
    pub fn named(name: ThemeName) -> Self {
        match name {
            ThemeName::Dark => Self::dark(),
            ThemeName::Light => Self::light(),
            ThemeName::Mono => Self::mono(),
        }
    }

    fn dark() -> Self {
        Self {
            primary: Color::Rgb(16, 185, 129), // emerald-500
            border: Color::Rgb(60, 60, 65),
            text: Color::Rgb(220, 220, 220),
            text_secondary: Color::Rgb(140, 140, 145),
            text_muted: Color::Rgb(80, 80, 85),
            success: Color::Rgb(34, 197, 94), // green-500
            warning: Color::Rgb(234, 179, 8), // yellow-500
            error: Color::Rgb(239, 68, 68),   // red-500
            info: Color::Rgb(96, 165, 250),   // blue-400
            selection_bg: Color::Rgb(30, 64, 110),
            selection_fg: Color::White,
            dev_server: Color::Rgb(34, 197, 94), // green
            database: Color::Rgb(234, 179, 8),   // yellow
            cache: Color::Rgb(168, 85, 247),     // purple
            queue: Color::Rgb(244, 114, 182),    // pink
            container: Color::Rgb(96, 165, 250), // blue
            browser: Color::Rgb(251, 146, 60),   // orange
            system: Color::Rgb(140, 140, 145),   // gray
            mono: false,
        }
    }

    fn light() -> Self {
        Self {
            primary: Color::Rgb(5, 150, 105), // emerald-600
            border: Color::Rgb(209, 213, 219),
            text: Color::Rgb(31, 41, 55),
            text_secondary: Color::Rgb(75, 85, 99),
            text_muted: Color::Rgb(107, 114, 128),
            success: Color::Rgb(22, 163, 74), // green-600
            warning: Color::Rgb(217, 119, 6), // amber-600
            error: Color::Rgb(220, 38, 38),   // red-600
            info: Color::Rgb(37, 99, 235),    // blue-600
            selection_bg: Color::Rgb(219, 234, 254),
            selection_fg: Color::Rgb(17, 24, 39),
            dev_server: Color::Rgb(21, 128, 61), // green-700
            database: Color::Rgb(180, 83, 9),    // amber-700
            cache: Color::Rgb(126, 34, 206),     // purple-700
            queue: Color::Rgb(190, 24, 93),      // pink-700
            container: Color::Rgb(29, 78, 216),  // blue-700
            browser: Color::Rgb(194, 65, 12),    // orange-700
            system: Color::Rgb(75, 85, 99),      // gray-600
            mono: false,
        }
    }

    fn mono() -> Self {
        let r = Color::Reset;
        Self {
            primary: r,
            border: r,
            text: r,
            text_secondary: r,
            text_muted: r,
            success: r,
            warning: r,
            error: r,
            info: r,
            selection_bg: r,
            selection_fg: r,
            dev_server: r,
            database: r,
            cache: r,
            queue: r,
            container: r,
            browser: r,
            system: r,
            mono: true,
        }
    }

    /// Downgrade every RGB colour to the nearest xterm-256 colour when needed.
    pub fn with_depth(mut self, depth: ColorDepth) -> Self {
        if depth == ColorDepth::Ansi256 {
            for c in [
                &mut self.primary,
                &mut self.border,
                &mut self.text,
                &mut self.text_secondary,
                &mut self.text_muted,
                &mut self.success,
                &mut self.warning,
                &mut self.error,
                &mut self.info,
                &mut self.selection_bg,
                &mut self.selection_fg,
                &mut self.dev_server,
                &mut self.database,
                &mut self.cache,
                &mut self.queue,
                &mut self.container,
                &mut self.browser,
                &mut self.system,
            ] {
                if let Color::Rgb(r, g, b) = *c {
                    *c = Color::Indexed(rgb_to_ansi256(r, g, b));
                }
            }
        }
        self
    }

    pub fn category_color(&self, category: ServiceCategory) -> Color {
        match category {
            ServiceCategory::DevServer => self.dev_server,
            ServiceCategory::Database => self.database,
            ServiceCategory::Cache => self.cache,
            ServiceCategory::Queue => self.queue,
            ServiceCategory::Container => self.container,
            ServiceCategory::Browser => self.browser,
            ServiceCategory::System => self.system,
            ServiceCategory::Unknown => self.text,
        }
    }

    /// Category color as crossterm Color (for non-TUI output)
    pub fn category_ct_color(&self, category: ServiceCategory) -> CtColor {
        to_ct(self.category_color(category))
    }

    /// Protocol color as crossterm Color (for non-TUI output)
    pub fn proto_ct_color(&self, protocol: Protocol) -> CtColor {
        match protocol {
            Protocol::Tcp => to_ct(self.info),
            Protocol::Udp => to_ct(self.text_muted),
        }
    }

    /// Get a crossterm Color from a theme field
    pub fn ct(&self, color: Color) -> CtColor {
        to_ct(color)
    }
}

/// Convert a ratatui colour to the crossterm colour used for CLI output.
fn to_ct(color: Color) -> CtColor {
    match color {
        Color::Rgb(r, g, b) => CtColor::Rgb { r, g, b },
        Color::Indexed(i) => CtColor::AnsiValue(i),
        Color::White => CtColor::White,
        _ => CtColor::Reset,
    }
}

/// Nearest xterm-256 colour: the 6×6×6 cube (16–231) or the greyscale ramp (232–255).
pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: u8| {
        (0..6)
            .min_by_key(|&i| (LEVELS[i] - i32::from(v)).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let dist = |(x, y, z): (i32, i32, i32)| {
        (x - i32::from(r)).pow(2) + (y - i32::from(g)).pow(2) + (z - i32::from(b)).pow(2)
    };
    let cube = dist((LEVELS[ri], LEVELS[gi], LEVELS[bi]));
    let avg = (i32::from(r) + i32::from(g) + i32::from(b)) / 3;
    let gray_index = ((avg - 8 + 5) / 10).clamp(0, 23);
    let gray_value = 8 + 10 * gray_index;
    if dist((gray_value, gray_value, gray_value)) < cube {
        (232 + gray_index) as u8
    } else {
        (16 + 36 * ri + 6 * gi + bi) as u8
    }
}

/// Colour depth from `COLORTERM`, `TERM` and `TERM_PROGRAM`.
pub fn detect_depth(
    colorterm: Option<&str>,
    term: Option<&str>,
    term_program: Option<&str>,
) -> ColorDepth {
    let truecolor = colorterm
        .is_some_and(|c| c.eq_ignore_ascii_case("truecolor") || c.eq_ignore_ascii_case("24bit"))
        || term_program.is_some_and(|p| {
            matches!(
                p,
                "iTerm.app" | "WezTerm" | "vscode" | "ghostty" | "WarpTerminal" | "Hyper"
            )
        })
        || term.is_some_and(|t| {
            t.contains("truecolor")
                || t.contains("direct")
                || t.starts_with("xterm-kitty")
                || t.starts_with("alacritty")
                || t.contains("ghostty")
        });
    if truecolor {
        ColorDepth::TrueColor
    } else {
        ColorDepth::Ansi256
    }
}

/// `COLORFGBG` (`fg;bg` or `fg;default;bg`): background 7 or 15 means light.
fn colorfgbg_is_light(value: &str) -> Option<bool> {
    let bg: u8 = value.rsplit(';').next()?.trim().parse().ok()?;
    Some(matches!(bg, 7 | 15))
}

/// Pick dark or light for `--theme auto`: `COLORFGBG` first, then `query`
/// (asks the terminal; `Some(true)` = light), otherwise dark.
pub fn resolve_auto(colorfgbg: Option<&str>, query: impl FnOnce() -> Option<bool>) -> ThemeName {
    let light = colorfgbg.and_then(colorfgbg_is_light).or_else(query);
    if light == Some(true) {
        ThemeName::Light
    } else {
        ThemeName::Dark
    }
}

/// The palette for a `--theme` choice; without colour it is always mono.
pub fn select(
    choice: ThemeChoice,
    color: bool,
    colorfgbg: Option<&str>,
    query: impl FnOnce() -> Option<bool>,
) -> ThemeName {
    if !color {
        return ThemeName::Mono;
    }
    match choice {
        ThemeChoice::Dark => ThemeName::Dark,
        ThemeChoice::Light => ThemeName::Light,
        ThemeChoice::Auto => resolve_auto(colorfgbg, query),
    }
}

/// Ask the terminal for its background (OSC 11), only on an interactive terminal.
fn query_terminal() -> Option<bool> {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return None;
    }
    let mut options = terminal_colorsaurus::QueryOptions::default();
    options.timeout = Duration::from_millis(250);
    match terminal_colorsaurus::theme_mode(options).ok()? {
        terminal_colorsaurus::ThemeMode::Light => Some(true),
        terminal_colorsaurus::ThemeMode::Dark => Some(false),
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn env_depth() -> ColorDepth {
    detect_depth(
        env("COLORTERM").as_deref(),
        env("TERM").as_deref(),
        env("TERM_PROGRAM").as_deref(),
    )
}

static THEME: OnceLock<Theme> = OnceLock::new();

/// Choose the theme once at startup (call before anything draws).
pub fn init(choice: ThemeChoice, color: bool) {
    let name = select(choice, color, env("COLORFGBG").as_deref(), query_terminal);
    let _ = THEME.set(Theme::named(name).with_depth(env_depth()));
}

/// Global theme instance (dark if `init` was never called, e.g. in tests).
pub fn theme() -> &'static Theme {
    THEME.get_or_init(|| Theme::dark().with_depth(env_depth()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color) -> f64 {
        let Color::Rgb(r, g, b) = c else {
            panic!("expected an RGB colour, got {c:?}")
        };
        let lin = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    #[test]
    fn colorfgbg_light_and_dark() {
        assert_eq!(resolve_auto(Some("0;15"), || None), ThemeName::Light);
        assert_eq!(resolve_auto(Some("0;7"), || None), ThemeName::Light);
        assert_eq!(resolve_auto(Some("15;0"), || None), ThemeName::Dark);
        assert_eq!(resolve_auto(Some("15;default;0"), || None), ThemeName::Dark);
        assert_eq!(
            resolve_auto(Some("0;15"), || panic!("COLORFGBG answers first")),
            ThemeName::Light
        );
    }

    #[test]
    fn auto_uses_the_terminal_answer_then_falls_back_to_dark() {
        assert_eq!(resolve_auto(None, || Some(true)), ThemeName::Light);
        assert_eq!(resolve_auto(None, || Some(false)), ThemeName::Dark);
        assert_eq!(resolve_auto(None, || None), ThemeName::Dark);
        assert_eq!(resolve_auto(Some("garbage"), || None), ThemeName::Dark);
    }

    #[test]
    fn explicit_choice_and_no_color() {
        assert_eq!(
            select(ThemeChoice::Light, true, None, || None),
            ThemeName::Light
        );
        assert_eq!(
            select(ThemeChoice::Dark, true, Some("0;15"), || None),
            ThemeName::Dark
        );
        assert_eq!(
            select(ThemeChoice::Light, false, None, || None),
            ThemeName::Mono
        );
        assert_eq!(
            select(ThemeChoice::Auto, false, None, || None),
            ThemeName::Mono
        );
    }

    #[test]
    fn ansi256_conversion_known_values() {
        assert_eq!(rgb_to_ansi256(255, 255, 255), 231);
        assert_eq!(rgb_to_ansi256(0, 0, 0), 16);
        assert_eq!(
            rgb_to_ansi256(16, 185, 129),
            36,
            "emerald → 6x6x6 cube (0,3,2)"
        );
        assert_eq!(
            rgb_to_ansi256(128, 128, 128),
            244,
            "mid grey → greyscale ramp"
        );
        assert_eq!(rgb_to_ansi256(255, 0, 0), 196);
    }

    #[test]
    fn depth_truecolor_from_colorterm() {
        assert_eq!(
            detect_depth(Some("truecolor"), None, None),
            ColorDepth::TrueColor
        );
        assert_eq!(
            detect_depth(Some("24bit"), None, None),
            ColorDepth::TrueColor
        );
        assert_eq!(
            detect_depth(None, None, Some("iTerm.app")),
            ColorDepth::TrueColor
        );
        assert_eq!(
            detect_depth(None, Some("xterm-kitty"), None),
            ColorDepth::TrueColor
        );
    }

    #[test]
    fn depth_256_for_apple_terminal() {
        assert_eq!(
            detect_depth(None, Some("xterm-256color"), Some("Apple_Terminal")),
            ColorDepth::Ansi256
        );
        assert_eq!(detect_depth(None, None, None), ColorDepth::Ansi256);
    }

    #[test]
    fn light_theme_text_is_dark() {
        let light = Theme::named(ThemeName::Light);
        assert!(luminance(light.text) < 0.3, "light theme text must be dark");
        let dark = Theme::named(ThemeName::Dark);
        assert!(luminance(dark.text) > 0.5, "dark theme text must be light");
    }

    #[test]
    fn ansi256_depth_converts_every_colour() {
        let t = Theme::named(ThemeName::Dark).with_depth(ColorDepth::Ansi256);
        assert!(matches!(t.primary, Color::Indexed(_)));
        assert!(matches!(t.ct(t.text), CtColor::AnsiValue(_)));
        assert!(matches!(
            t.category_ct_color(ServiceCategory::Queue),
            CtColor::AnsiValue(_)
        ));
    }

    #[test]
    fn mono_theme_uses_terminal_defaults() {
        let t = Theme::named(ThemeName::Mono);
        assert!(t.mono);
        assert_eq!(t.text, Color::Reset);
        assert_eq!(t.ct(t.text), CtColor::Reset);
    }
}
