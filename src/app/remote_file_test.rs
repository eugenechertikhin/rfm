//! Тесты view/edit через временную копию: записи архива (без сети), заливка после
//! сохранения/внешнего редактора, ошибки (архив только для чтения, архивы на FTP).

use super::*;
use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn tmp() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("rfm_rfile_test_{}_{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn chord(app: &mut App, c: char) {
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char(c)));
}

fn cursor_to(app: &mut App, name: &str) {
    let idx = app.active_panel().entries.iter().position(|e| e.name == name).unwrap();
    app.active_panel_mut().cursor = idx;
}

/// Приложение, открытое внутри локального tar-архива с файлом `a.txt`.
fn app_in_tar() -> (App, PathBuf) {
    let d = tmp();
    fs::create_dir_all(d.join("src")).unwrap();
    fs::write(d.join("src/a.txt"), b"hello\n").unwrap();
    let tar = d.join("t.tar");
    assert!(std::process::Command::new("tar")
        .arg("-cf").arg(&tar).arg("-C").arg(d.join("src")).arg("a.txt")
        .status().unwrap().success());
    let mut app = App::new(Config::default());
    app.active_panel_mut().path.set_local(d.clone());
    app.reload();
    let cfg = crate::config::ArchiveConfig {
        name: "tar".into(),
        extensions: vec!["tar".into()],
        list_command: "tar -tvf".into(),
    };
    app.open_archive(&tar, &cfg);
    cursor_to(&mut app, "a.txt");
    (app, d)
}

#[test]
fn view_archive_member_via_temp_and_cleanup_on_close() {
    let (mut app, d) = app_in_tar();
    chord(&mut app, 'v');
    let tmp_path = {
        let v = app.active_panel_mut().viewer.as_mut().expect("viewer");
        assert!(v.name.ends_with("t.tar:/a.txt"), "{}", v.name);
        assert_eq!(v.window(0, 1), vec!["hello".to_string()]);
        v.copy.as_ref().expect("temp copy").path.clone()
    };
    assert!(tmp_path.exists());
    app.handle_key(key(KeyCode::Esc));
    assert!(app.active_panel().viewer.is_none());
    assert!(!tmp_path.exists());
    fs::remove_dir_all(&d).ok();
}

#[test]
fn prettify_keeps_temp_copy() {
    let (mut app, d) = app_in_tar();
    chord(&mut app, 'v');
    app.handle_key(key(KeyCode::Char('p'))); // .txt — unsupported, вид не меняется
    assert!(app.active_panel().viewer.as_ref().unwrap().copy.is_some());
    fs::remove_dir_all(&d).ok();
}

#[test]
fn edit_archive_member_is_read_only() {
    let (mut app, d) = app_in_tar();
    chord(&mut app, 'e');
    assert!(app.active_panel().editor.is_none());
    assert_eq!(app.status, "edit: archive is read-only");
    fs::remove_dir_all(&d).ok();
}

/// Редактор над временной копией локального файла (origin — сам файл).
fn app_editing_copy(d: &std::path::Path) -> (App, PathBuf) {
    let orig = d.join("f.txt");
    fs::write(&orig, b"abc\n").unwrap();
    let copy = LocalCopy::fetch(Item { loc: Loc::Local(orig.clone()), is_dir: false }, "edit").unwrap();
    let mut app = App::new(Config::default());
    let path = copy.path.clone();
    app.open_editor(&path);
    app.attach_copy(copy, true);
    (app, orig)
}

#[test]
fn editor_save_uploads_copy_to_origin() {
    let d = tmp();
    let (mut app, orig) = app_editing_copy(&d);
    assert_eq!(app.active_panel().editor.as_ref().unwrap().name, orig.display().to_string());
    app.handle_key(key(KeyCode::Char('X')));
    chord(&mut app, 's');
    assert_eq!(app.status, "saved");
    assert_eq!(fs::read(&orig).unwrap(), b"Xabc\n");
    assert!(!app.active_panel().editor.as_ref().unwrap().dirty);
    fs::remove_dir_all(&d).ok();
}

#[test]
fn editor_upload_failure_keeps_dirty() {
    let d = tmp();
    let (mut app, _orig) = app_editing_copy(&d);
    {
        let e = app.active_panel_mut().editor.as_mut().unwrap();
        let c = e.copy.as_mut().unwrap();
        c.origin.loc = Loc::Archive(
            crate::vfs::transfer::ArchiveSrc { file: d.join("x.tar"), kind: "tar".into(), remote: None },
            "f.txt".into(),
        );
    }
    app.handle_key(key(KeyCode::Char('X')));
    chord(&mut app, 's');
    assert_eq!(app.status, "edit: upload failed: archive is read-only");
    assert!(app.active_panel().editor.as_ref().unwrap().dirty);
    fs::remove_dir_all(&d).ok();
}

#[test]
fn after_shell_uploads_only_changed_copy() {
    let d = tmp();
    let orig = d.join("f.txt");
    fs::write(&orig, b"one").unwrap();
    let mut app = App::new(Config::default());
    let fetch = || LocalCopy::fetch(Item { loc: Loc::Local(orig.clone()), is_dir: false }, "edit").unwrap();

    // Не менялась — не заливаем, временная копия удаляется.
    let copy = fetch();
    let tmp_path = copy.path.clone();
    app.pending_external = Some(ExternalEdit { before: copy.stamp(), copy, edit: true });
    app.after_shell();
    assert!(app.status.is_empty());
    assert!(!tmp_path.exists());

    // Изменилась — заливаем.
    let copy = fetch();
    let before = copy.stamp();
    fs::write(&copy.path, b"changed").unwrap();
    app.pending_external = Some(ExternalEdit { before, copy, edit: true });
    app.after_shell();
    assert_eq!(app.status, format!("uploaded {}", orig.display()));
    assert_eq!(fs::read(&orig).unwrap(), b"changed");
    assert!(app.pending_external.is_none());

    // Просмотр (не edit) — не заливаем даже при изменении.
    let copy = fetch();
    let before = copy.stamp();
    fs::write(&copy.path, b"viewer touched").unwrap();
    app.status.clear();
    app.pending_external = Some(ExternalEdit { before, copy, edit: false });
    app.after_shell();
    assert_eq!(fs::read(&orig).unwrap(), b"changed");
    fs::remove_dir_all(&d).ok();
}

#[test]
fn archive_on_ftp_is_not_supported() {
    let mut app = App::new(Config::default());
    app.config.archives = crate::config::builtin_archives();
    let t = crate::vfs::ftp::FtpTarget { user: None, host: "h".into(), port: None, path: "/".into() };
    app.active_panel_mut().path.push_ftp(t, None, "/".into());
    app.active_panel_mut().entries = vec![VfsEntry {
        name: "x.zip".to_string(),
        kind: crate::vfs::EntryKind::File,
        size: Some(1),
        permissions: None,
        owner: None,
        group: None,
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: false,
        depth: 0,
    }];
    app.active_panel_mut().cursor = 0;
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.status, "archive is not supported on ftp");
}
