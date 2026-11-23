//! Юнит-тесты встроенного просмотрщика

use super::*;
use std::path::PathBuf;

/// Приложение с просмотрщиком файла во временной директории.
fn app_with_viewer() -> (App, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rfm_viewer_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("f.txt");
    std::fs::write(&path, "hello\n").unwrap();
    let mut app = App::new(crate::config::Config::default());
    app.open_viewer(&path);
    assert!(app.active_panel().viewer.is_some());
    (app, dir)
}

fn chord(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

#[test]
fn ctrl_x_h_opens_viewer_help() {
    let (mut app, dir) = app_with_viewer();
    chord(&mut app, 'h');
    let help = app.help.as_ref().expect("help open");
    assert_eq!(help.name, "Viewer help");
    let text: Vec<String> = help.content_lines().cloned().unwrap_or_default();
    assert!(text.iter().any(|l| l.contains("toggle hex dump")));
    assert!(text.iter().any(|l| l.contains("start search")));
    assert!(!text.iter().any(|l| l.contains("Panels:"))); // не общий help

    // Закрытие помощи возвращает в просмотрщик.
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.help.is_none());
    assert!(app.active_panel().viewer.is_some());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_x_h_outside_viewer_opens_general_help() {
    let mut app = App::new(crate::config::Config::default());
    chord(&mut app, 'h');
    let help = app.help.as_ref().expect("help open");
    assert_eq!(help.name, "Help");
    let text: Vec<String> = help.content_lines().cloned().unwrap_or_default();
    assert!(text.iter().any(|l| l.contains("Panels:")));
}
