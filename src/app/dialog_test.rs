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

// ---- Назначение copy/move и операции на VFS ----

use crate::vfs::sftp::SftpTarget;
use crate::vfs::transfer::{Item, Loc};

fn starget(user: &str, path: &str) -> SftpTarget {
    SftpTarget { user: Some(user.into()), host: "h".into(), port: None, path: path.into() }
}

/// Две панели: активная — локальная (cwd), вторая — на SFTP `u@h:/srv`.
fn app_with_sftp_panel() -> App {
    let mut app = app_with_a();
    let mut p = Panel::new(app.panels[0].path.clone());
    p.path.push_sftp(starget("u", "/srv"), PathBuf::from("/sock"), "/srv".into());
    app.panels.push(p);
    app
}

#[test]
fn parse_dest_local_forms() {
    let app = app_with_a();
    let home = dirs::home_dir().unwrap();
    assert_eq!(app.parse_dest("/abs/x"), Ok(Loc::Local(PathBuf::from("/abs/x"))));
    assert_eq!(app.parse_dest("~"), Ok(Loc::Local(home.clone())));
    assert_eq!(app.parse_dest("~/d"), Ok(Loc::Local(home.join("d"))));
    let cwd = app.active_panel().path.local_path().unwrap().to_path_buf();
    assert_eq!(app.parse_dest("rel/y"), Ok(Loc::Local(cwd.join("rel/y"))));
}

#[test]
fn parse_dest_url_reuses_open_session_from_any_panel() {
    let app = app_with_sftp_panel();
    match app.parse_dest("sftp://u@h/tmp/x") {
        Ok(Loc::Sftp(c, p)) => {
            assert_eq!(p, "/tmp/x");
            assert_eq!(c.sock, PathBuf::from("/sock"));
        }
        other => panic!("sftp loc expected, got {other:?}"),
    }
    // другой пользователь — сессии нет
    assert_eq!(app.parse_dest("sftp://other@h/x"), Err("no open session: sftp://other@h/x".into()));
}

#[test]
fn parse_dest_relative_goes_to_remote_backend() {
    let mut app = app_with_sftp_panel();
    app.active = 1;
    assert!(matches!(app.parse_dest("321"), Ok(Loc::Sftp(_, ref p)) if p == "/srv/321"));
    // абсолютный путь и `~` — всегда локальные
    assert!(matches!(app.parse_dest("/tmp"), Ok(Loc::Local(_))));
}

#[test]
fn parse_dest_ftp_and_http() {
    let app = app_with_a();
    // анонимный FTP — соединение «на лету»
    assert!(matches!(app.parse_dest("ftp://host/pub"), Ok(Loc::Ftp(ref c, ref p)) if c.password.is_none() && p == "/pub"));
    assert_eq!(app.parse_dest("ftp://bob@host/x"), Err("no open session: ftp://bob@host/x".into()));
    assert_eq!(app.parse_dest("https://e.com/"), Err("http is read-only".into()));
    assert_eq!(app.parse_dest("gopher://x/"), Err("unsupported destination: gopher://x/".into()));
}

#[test]
fn dest_default_uses_remote_url_of_other_panel() {
    let app = app_with_sftp_panel();
    assert_eq!(app.dest_default(), "sftp://u@h/srv");
}

#[test]
fn archive_panel_is_read_only_but_copyable() {
    let mut app = app_with_a();
    let entries = app.active_panel().entries.clone();
    app.active_panel_mut().path.push_archive_items("x.zip", vec![]);
    app.active_panel_mut().entries = entries;
    for op in ['n', 'm', 'd'] {
        app.status.clear();
        app.handle_key(ctrl('x'));
        app.handle_key(key(KeyCode::Char(op)));
        assert!(app.dialog.is_none(), "op {op}");
        assert_eq!(app.status, "archive is read-only", "op {op}");
    }
    open(&mut app, 'c');
    match &app.dialog {
        Some(Dialog::Input { op: PendingOp::Copy(items), .. }) => {
            assert!(matches!(&items[0].loc, Loc::Archive(a, p) if p == "a" && a.kind == "zip"));
        }
        _ => panic!("copy dialog expected"),
    }
}

#[test]
fn copy_into_archive_destination_rejected() {
    let mut app = app_with_a();
    app.active_panel_mut().path.push_archive_items("x.zip", vec![]);
    app.execute_op(PendingOp::Copy(vec![Item::local(PathBuf::from("/etc/hosts"))]), Some(PathBuf::from("sub")));
    assert_eq!(app.status, "archive is read-only");
}
