use crossterm::style::Color as CtColor;
use ratatui::style::Color;

use crate::models::{Protocol, ServiceCategory};

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
    pub container: Color,
    pub browser: Color,
    pub system: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            primary: Color::Rgb(16, 185, 129),   // emerald-500
            border: Color::Rgb(60, 60, 65),
            text: Color::Rgb(220, 220, 220),
            text_secondary: Color::Rgb(140, 140, 145),
            text_muted: Color::Rgb(80, 80, 85),
            success: Color::Rgb(34, 197, 94),    // green-500
            warning: Color::Rgb(234, 179, 8),    // yellow-500
            error: Color::Rgb(239, 68, 68),      // red-500
            info: Color::Rgb(96, 165, 250),      // blue-400
            selection_bg: Color::Rgb(30, 64, 110),
            selection_fg: Color::White,
            dev_server: Color::Rgb(34, 197, 94), // green
            database: Color::Rgb(234, 179, 8),   // yellow
            cache: Color::Rgb(168, 85, 247),     // purple
            container: Color::Rgb(96, 165, 250), // blue
            browser: Color::Rgb(251, 146, 60),   // orange
            system: Color::Rgb(140, 140, 145),   // gray
        }
    }
}

/// Convert a ratatui Color::Rgb to crossterm Color::Rgb
fn to_ct(color: Color) -> CtColor {
    match color {
        Color::Rgb(r, g, b) => CtColor::Rgb { r, g, b },
        Color::White => CtColor::White,
        _ => CtColor::Reset,
    }
}

impl Theme {
    pub fn category_color(&self, category: ServiceCategory) -> Color {
        match category {
            ServiceCategory::DevServer => self.dev_server,
            ServiceCategory::Database => self.database,
            ServiceCategory::Cache => self.cache,
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
            Protocol::Tcp => to_ct(self.info), // cyan/blue
            Protocol::Udp => to_ct(self.text_muted),
        }
    }

    /// Get a crossterm Color from a theme field
    pub fn ct(&self, color: Color) -> CtColor {
        to_ct(color)
    }
}

/// Global theme instance
pub fn theme() -> &'static Theme {
    use std::sync::OnceLock;
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(Theme::default)
}
