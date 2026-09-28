//! Цветовая тема: дефолтная тёмная + сборка из конфига (`[[themes]]`).

use ratatui::style::Color;

use crate::config::ThemeConfig;

#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub cursor_bg: Color,
    pub cursor_fg: Color,
    pub mark_fg: Color,
    /// Фон/текст командной строки (внизу экрана).
    pub cmdline_bg: Color,
    pub cmdline_fg: Color,
}

impl Theme {
    /// Дефолт по уставу: тёмно-серый фон, белые буквы, светло-серый курсор с
    /// чёрными буквами под ним, помеченные файлы — жёлтые.
    pub fn default_dark() -> Self {
        Self {
            bg: Color::Rgb(38, 38, 38),
            fg: Color::Rgb(220, 220, 220),
            cursor_bg: Color::Rgb(180, 180, 180),
            cursor_fg: Color::Black,
            mark_fg: Color::Yellow,
            cmdline_bg: Color::Rgb(38, 38, 38),
            cmdline_fg: Color::Rgb(220, 220, 220),
        }
    }

    /// Собирает тему из конфига; неразобранные/пустые цвета берутся из дефолта.
    /// Цвета командной строки по умолчанию наследуют bg/fg темы.
    pub fn from_config(tc: &ThemeConfig) -> Self {
        let d = Self::default_dark();
        let bg = parse_color(&tc.bg).unwrap_or(d.bg);
        let fg = parse_color(&tc.fg).unwrap_or(d.fg);
        Self {
            bg,
            fg,
            cursor_bg: parse_color(&tc.cursor_bg).unwrap_or(d.cursor_bg),
            cursor_fg: parse_color(&tc.cursor_fg).unwrap_or(d.cursor_fg),
            mark_fg: parse_color(&tc.mark_fg).unwrap_or(d.mark_fg),
            cmdline_bg: parse_color(&tc.cmdline_bg).unwrap_or(bg),
            cmdline_fg: parse_color(&tc.cmdline_fg).unwrap_or(fg),
        }
    }
}

/// Разбирает цвет: `#RRGGBB` или именованный (`black`, `white`, `red`, `green`,
/// `yellow`, `blue`, `magenta`, `cyan`, `gray`/`grey`, а также `light*`-варианты).
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    Some(match s.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" | "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "theme_test.rs"]
mod tests;
