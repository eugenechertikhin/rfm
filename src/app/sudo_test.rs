//! Тесты команд sudo: локально, на SFTP (через `ssh -t`), отказ между хостами/на FTP.

use super::*;
use crate::vfs::sftp::SftpTarget;

fn starget(user: &str, host: &str, port: Option<u16>) -> SftpTarget {
    SftpTarget { user: Some(user.into()), host: host.into(), port, path: "/srv".into() }
}

/// Активная панель — на SFTP `u@h:/srv` (порт 2222).
fn app_on_sftp() -> App {
    let mut app = App::new(Config::default());
    app.active_panel_mut().path.push_sftp(starget("u", "h", Some(2222)), PathBuf::from("/sock"), "/srv".into());
    app
}

fn remote(app: &App, name: &str) -> Item {
    Item { loc: app.active_panel().path.cwd_loc().unwrap().join(name), is_dir: false }
}

#[test]
fn local_commands_unchanged() {
    let mut app = App::new(Config::default());
    let items = vec![Item::local(PathBuf::from("/etc/a b"))];
    assert_eq!(app.sudo_delete_cmd(&items).unwrap(), "sudo rm -rf -- '/etc/a b'");
    let op = PendingOp::Copy(vec![Item::local(PathBuf::from("/etc/hosts"))]);
    assert_eq!(app.sudo_op_cmd(&op, "/tmp").unwrap(), "sudo cp -r -- '/etc/hosts' '/tmp'");
    // dest — как введён (относительный — от cwd, где выполнится команда)
    let op = PendingOp::Move(vec![Item::local(PathBuf::from("/x"))]);
    assert_eq!(app.sudo_op_cmd(&op, "sub").unwrap(), "sudo mv -- '/x' 'sub'");
    assert_eq!(app.sudo_op_cmd(&op, "  "), None);
}

#[test]
fn sftp_delete_runs_on_server_with_tty() {
    let mut app = app_on_sftp();
    let items = vec![remote(&app, "a"), remote(&app, "it's")];
    assert_eq!(
        app.sudo_delete_cmd(&items).unwrap(),
        "ssh -t -S '/sock' -p 2222 'u@h' 'sudo rm -rf -- '\\''/srv/a'\\'' '\\''/srv/it'\\''\\'\\'''\\''s'\\'''"
    );
}

#[test]
fn sftp_copy_move_same_server() {
    let mut app = app_on_sftp();
    let op = PendingOp::Move(vec![remote(&app, "a")]);
    // относительное назначение — от директории панели на сервере
    let cmd = app.sudo_op_cmd(&op, "sub").unwrap();
    assert_eq!(cmd, "ssh -t -S '/sock' -p 2222 'u@h' 'sudo mv -- '\\''/srv/a'\\'' '\\''/srv/sub'\\'''");
    // URL того же сервера (сессия открыта)
    let op = PendingOp::Copy(vec![remote(&app, "a")]);
    let cmd = app.sudo_op_cmd(&op, "sftp://u@h:2222/etc").unwrap();
    assert!(cmd.ends_with("'sudo cp -r -- '\\''/srv/a'\\'' '\\''/etc'\\'''"), "{cmd}");
}

#[test]
fn sudo_rejected_between_hosts_and_ftp() {
    let mut app = app_on_sftp();
    // удалённый источник → локальное назначение (абсолютный путь — локальный)
    let op = PendingOp::Copy(vec![remote(&app, "a")]);
    assert_eq!(app.sudo_op_cmd(&op, "/tmp"), None);
    assert_eq!(app.status, "sudo is not supported between hosts");
    // локальный источник → sftp
    let op = PendingOp::Copy(vec![Item::local(PathBuf::from("/etc/hosts"))]);
    app.status.clear();
    assert_eq!(app.sudo_op_cmd(&op, "x"), None);
    assert_eq!(app.status, "sudo is not supported between hosts");
    // другой сервер в одной операции
    let other = Item {
        loc: Loc::Sftp(SftpConn { target: starget("u", "other", None), sock: "/s2".into() }, "/f".into()),
        is_dir: false,
    };
    assert_eq!(app.sudo_delete_cmd(&[remote(&app, "a"), other]), None);
    assert_eq!(app.status, "sudo is not supported between hosts");
    // нет открытой сессии для URL
    assert_eq!(app.sudo_op_cmd(&op, "sftp://z@nowhere/x"), None);
    assert_eq!(app.status, "no open session: sftp://z@nowhere/x");
    // ftp
    let ftp = Item {
        loc: Loc::Ftp(
            crate::vfs::transfer::FtpConn {
                target: crate::vfs::ftp::FtpTarget { user: None, host: "f".into(), port: None, path: "/".into() },
                password: None,
            },
            "/a".into(),
        ),
        is_dir: false,
    };
    assert_eq!(app.sudo_delete_cmd(&[ftp]), None);
    assert_eq!(app.status, "sudo is not supported for ftp");
}

#[test]
fn ctrl_s_in_delete_dialog_on_sftp_returns_ssh_command() {
    let mut app = app_on_sftp();
    app.active_panel_mut().entries = vec![VfsEntry {
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
    }];
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    match app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)) {
        Action::RunShell(cmd) => assert!(cmd.starts_with("ssh -t -S '/sock'"), "{cmd}"),
        other => panic!("expected RunShell, got {other:?}"),
    }
}
