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
    /// Фон/текст кнопки диалога в фокусе.
    pub button_sel_bg: Color,
    pub button_sel_fg: Color,
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
            button_sel_bg: Color::Black,
            button_sel_fg: Color::Rgb(220, 220, 220),
        }
    }

    /// Собирает тему из конфига; любой пустой/неразобранный цвет берётся
    /// из дефолтной темы (той, что применяется при старте без конфига).
    pub fn from_config(tc: &ThemeConfig) -> Self {
        let d = Self::default_dark();
        let c = |s: &str, dflt: Color| parse_color(s).unwrap_or(dflt);
        Self {
            bg: c(&tc.bg, d.bg),
            fg: c(&tc.fg, d.fg),
            cursor_bg: c(&tc.cursor_bg, d.cursor_bg),
            cursor_fg: c(&tc.cursor_fg, d.cursor_fg),
            mark_fg: c(&tc.mark_fg, d.mark_fg),
            cmdline_bg: c(&tc.cmdline_bg, d.cmdline_bg),
            cmdline_fg: c(&tc.cmdline_fg, d.cmdline_fg),
            button_sel_bg: c(&tc.button_sel_bg, d.button_sel_bg),
            button_sel_fg: c(&tc.button_sel_fg, d.button_sel_fg),
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
