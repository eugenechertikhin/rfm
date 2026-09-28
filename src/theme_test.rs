//! Юнит-тесты модуля `theme` (в отдельном файле — по уставу).

use super::*;

#[test]
fn parse_hex_and_named() {
    assert_eq!(parse_color("#ff0000"), Some(Color::Rgb(255, 0, 0)));
    assert_eq!(parse_color("yellow"), Some(Color::Yellow));
    assert_eq!(parse_color("GRAY"), Some(Color::DarkGray));
    assert_eq!(parse_color(""), None);
    assert_eq!(parse_color("#zzz"), None);
    assert_eq!(parse_color("notacolor"), None);
}

#[test]
fn from_config_falls_back_on_bad_values() {
    let d = Theme::default_dark();
    let tc = ThemeConfig {
        name: "x".to_string(),
        bg: "#000000".to_string(),
        fg: "".to_string(),          // fallback
        cursor_bg: "bogus".to_string(), // fallback
        cursor_fg: "white".to_string(),
        mark_fg: "#00ff00".to_string(),
        cmdline_bg: String::new(),
        cmdline_fg: String::new(),
    };
    let t = Theme::from_config(&tc);
    assert_eq!(t.bg, Color::Rgb(0, 0, 0));
    assert_eq!(t.fg, d.fg); // из дефолта
    assert_eq!(t.cursor_bg, d.cursor_bg); // из дефолта
    assert_eq!(t.cursor_fg, Color::White);
    assert_eq!(t.mark_fg, Color::Rgb(0, 255, 0));
}
