//! Юнит-тесты поиска/замены во встроенном редакторе

use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn app_with_editor(content: &str) -> (App, PathBuf) {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rfm_find_test_{}_{}", std::process::id(), n));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("f.txt");
    fs::write(&path, content).unwrap();
    let mut app = App::new(crate::config::Config::default());
    app.active_panel_mut().editor = Some(Editor::load(&path).unwrap());
    (app, dir)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn ed(app: &App) -> &Editor {
    app.active_panel().editor.as_ref().unwrap()
}

fn pos(app: &App) -> (usize, usize) {
    (ed(app).cur_line, ed(app).cur_col)
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

/// Открывает `Ctrl+r`, заполняет оба поля (очищая префилл) и оставляет диалог открытым.
fn fill_replace(app: &mut App, find: &str, repl: &str) {
    app.handle_key(ctrl('r'));
    assert!(matches!(app.dialog, Some(Dialog::Replace { field: 0, .. })));
    app.handle_key(ctrl('u'));
    type_str(app, find);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(ctrl('e'));
    app.handle_key(ctrl('u'));
    type_str(app, repl);
}

// ---- Editor: замена ----

#[test]
fn replace_one_case_insensitive_cursor_after_inserted() {
    let (mut app, dir) = app_with_editor("xx Foo foo\n");
    let e = app.active_panel_mut().editor.as_mut().unwrap();
    assert!(e.replace_one("FOO", "bar!"));
    assert_eq!(e.lines, vec!["xx bar! foo"]);
    assert_eq!((e.cur_line, e.cur_col), (0, 7));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn replace_one_wraps_around_and_reports_missing() {
    let (mut app, dir) = app_with_editor("ab\ncd\n");
    let e = app.active_panel_mut().editor.as_mut().unwrap();
    e.cur_line = 1;
    assert!(e.replace_one("a", "A"));
    assert_eq!(e.lines, vec!["Ab", "cd"]);
    assert_eq!((e.cur_line, e.cur_col), (0, 1));
    assert!(!e.replace_one("zz", "y"));
    assert!(!e.replace_one("", "y"));
    assert_eq!(e.lines, vec!["Ab", "cd"]);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn replace_all_counts_non_overlapping_and_clamps_cursor() {
    let (mut app, dir) = app_with_editor("aaa x\nAAAA\n");
    let e = app.active_panel_mut().editor.as_mut().unwrap();
    e.cur_line = 1;
    e.cur_col = 4;
    assert_eq!(e.replace_all("aa", "b"), 3);
    assert_eq!(e.lines, vec!["ba x", "bb"]);
    assert_eq!((e.cur_line, e.cur_col), (1, 2));
    assert_eq!(e.replace_all("zz", "b"), 0);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn replace_multibyte_safe() {
    let (mut app, dir) = app_with_editor("привет мир\n");
    let e = app.active_panel_mut().editor.as_mut().unwrap();
    assert!(e.replace_one("МИР", "world"));
    assert_eq!(e.lines, vec!["привет world"]);
    assert_eq!(e.cur_col, 12);
    fs::remove_dir_all(&dir).ok();
}

// ---- Диалог Ctrl+r ----

#[test]
fn ctrl_y_replaces_next_and_closes_then_reopen_prefilled() {
    let (mut app, dir) = app_with_editor("foo foo\n");
    fill_replace(&mut app, "foo", "X");
    app.handle_key(ctrl('y'));
    assert!(app.dialog.is_none());
    assert_eq!(ed(&app).lines, vec!["X foo"]);
    assert_eq!(pos(&app), (0, 1));
    assert!(ed(&app).dirty);
    assert_eq!(app.status, "replaced 1");
    // Повторно: префилл из прошлого раза, Enter — следующая замена.
    app.handle_key(ctrl('r'));
    match &app.dialog {
        Some(Dialog::Replace { find, repl, field }) => {
            assert_eq!((find.text().as_str(), repl.text().as_str(), *field), ("foo", "X", 0));
        }
        _ => panic!("replace dialog expected"),
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(ed(&app).lines, vec!["X X"]);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_l_replaces_all_with_count() {
    let (mut app, dir) = app_with_editor("a b a\na\n");
    fill_replace(&mut app, "A", "");
    app.handle_key(ctrl('l'));
    assert!(app.dialog.is_none());
    assert_eq!(ed(&app).lines, vec![" b ", ""]);
    assert_eq!(app.status, "replaced 3");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn tab_focuses_replace_all_and_enter_presses_it() {
    let (mut app, dir) = app_with_editor("a a\n");
    fill_replace(&mut app, "a", "b");
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(ed(&app).lines, vec!["b b"]);
    assert_eq!(app.status, "replaced 2");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn not_found_and_empty_search_leave_text_untouched() {
    let (mut app, dir) = app_with_editor("abc\n");
    fill_replace(&mut app, "zz", "y");
    app.handle_key(ctrl('y'));
    assert_eq!(app.status, "not found: zz");
    fill_replace(&mut app, "", "y");
    app.handle_key(ctrl('l'));
    assert_eq!(app.status, "empty search");
    assert_eq!(ed(&app).lines, vec!["abc"]);
    assert!(!ed(&app).dirty);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn cancel_keeps_text_and_esc_too() {
    let (mut app, dir) = app_with_editor("abc\n");
    fill_replace(&mut app, "a", "b");
    app.handle_key(ctrl('n'));
    assert!(app.dialog.is_none());
    assert_eq!(app.status, "cancelled");
    fill_replace(&mut app, "a", "b");
    app.handle_key(key(KeyCode::Esc));
    assert!(app.dialog.is_none());
    assert_eq!(ed(&app).lines, vec!["abc"]);
    assert!(app.active_panel().editor.is_some()); // редактор не закрылся
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn up_down_switch_fields_and_edit_only_active() {
    let (mut app, dir) = app_with_editor("abc\n");
    app.handle_key(ctrl('r'));
    type_str(&mut app, "ab");
    app.handle_key(key(KeyCode::Down));
    type_str(&mut app, "xy");
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(ctrl('a'));
    type_str(&mut app, "z");
    match &app.dialog {
        Some(Dialog::Replace { find, repl, field }) => {
            assert_eq!((find.text().as_str(), repl.text().as_str(), *field), ("zab", "x", 0));
        }
        _ => panic!("replace dialog expected"),
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn replace_shares_search_string_with_ctrl_n() {
    let (mut app, dir) = app_with_editor("q w q\n");
    fill_replace(&mut app, "q", "Q");
    app.handle_key(ctrl('y'));
    assert_eq!(pos(&app), (0, 1));
    app.handle_key(ctrl('n')); // регистронезависимо: следующее «q» — в колонке 4
    assert_eq!(pos(&app), (0, 4));
    fs::remove_dir_all(&dir).ok();
}
