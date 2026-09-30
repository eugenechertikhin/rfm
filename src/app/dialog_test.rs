//! Тесты диалогов: фокус кнопок (`Tab`) и `Enter` по кнопке в фокусе.

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Приложение с одним файлом `a` под курсором.
fn app_with_a() -> App {
    let mut app = App::new(Config::default());
    let e = VfsEntry {
        name: "a".to_string(),
        kind: crate::vfs::EntryKind::File,
        size: Some(0),
        permissions: None,
        owner: None,
        group: None,
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: false,
        depth: 0,
    };
    app.active_panel_mut().entries = vec![e];
    app.active_panel_mut().cursor = 0;
    app
}

fn open(app: &mut App, op: char) {
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char(op)));
    assert!(app.dialog.is_some());
}

#[test]
fn buttons_per_dialog() {
    let labels = |d: &Dialog| dialog_buttons(d).iter().map(|(l, _)| *l).collect::<Vec<_>>();
    let quit = Dialog::Confirm { message: String::new(), op: PendingOp::QuitEditor };
    assert_eq!(labels(&quit), ["[ Yes (c-y) ]", "[ Cancel (c-n) ]"]);
    let del = Dialog::Confirm { message: String::new(), op: PendingOp::Delete(vec![]) };
    assert_eq!(labels(&del), ["[ Delete (c-y) ]", "[ Sudo (c-s) ]", "[ Cancel (c-n) ]"]);
    let hist = Dialog::HistorySearch { input: CmdLine::default(), results: vec![], sel: 0 };
    assert!(labels(&hist).is_empty());
}

#[test]
fn tab_cycles_focus_and_wraps() {
    let mut app = app_with_a();
    open(&mut app, 'c'); // copy: Copy / Sudo / Cancel
    assert_eq!(app.dialog_btn, 0);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.dialog_btn, 1);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.dialog_btn, 2);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.dialog_btn, 0);
    assert!(app.dialog.is_some()); // Tab ничего не нажимает
}

#[test]
fn enter_presses_focused_sudo() {
    let mut app = app_with_a();
    open(&mut app, 'm');
    app.handle_key(key(KeyCode::Tab)); // → Sudo
    match app.handle_key(key(KeyCode::Enter)) {
        Action::RunShell(cmd) => assert!(cmd.starts_with("sudo mv --"), "cmd was {cmd:?}"),
        other => panic!("expected RunShell, got {other:?}"),
    }
    assert!(app.dialog.is_none());
    assert_eq!(app.dialog_btn, 0); // сброшен для следующего диалога
}

#[test]
fn enter_presses_focused_cancel_in_delete() {
    let mut app = app_with_a();
    open(&mut app, 'd');
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Tab)); // → Cancel
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
    assert_eq!(app.status, "cancelled");
    assert_eq!(app.dialog_btn, 0);
}

#[test]
fn enter_without_tab_is_first_button() {
    let mut app = app_with_a();
    open(&mut app, 'n'); // mkdir: OK / Cancel
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Tab)); // по кругу обратно на OK
    assert_eq!(app.dialog_btn, 0);
    app.handle_key(key(KeyCode::Esc));
    // Новый диалог — фокус снова на первой кнопке.
    open(&mut app, 'n');
    assert_eq!(app.dialog_btn, 0);
}

#[test]
fn shortcuts_ignore_focus() {
    let mut app = app_with_a();
    open(&mut app, 'c');
    app.handle_key(key(KeyCode::Tab)); // фокус на Sudo
    app.handle_key(ctrl('n')); // но c-n — всё равно отмена
    assert!(app.dialog.is_none());
    assert_eq!(app.status, "cancelled");
}

#[test]
fn quit_editor_tab_to_cancel_stays() {
    let mut app = app_with_a();
    app.dialog = Some(Dialog::Confirm { message: String::new(), op: PendingOp::QuitEditor });
    app.handle_key(key(KeyCode::Tab)); // → Cancel
    app.handle_key(key(KeyCode::Tab)); // → Yes (по кругу, 2 кнопки)
    assert_eq!(app.dialog_btn, 0);
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Enter)); // Cancel
    assert!(app.dialog.is_none());
    assert_eq!(app.status, "cancelled");
}

#[test]
fn tab_in_dialog_without_buttons_is_ignored() {
    let mut app = app_with_a();
    app.dialog = Some(Dialog::HistorySearch { input: CmdLine::default(), results: vec![], sel: 0 });
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.dialog_btn, 0);
    assert!(app.dialog.is_some());
}
