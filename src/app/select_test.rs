//! Тесты пометки по маске (`+`/`-`).

use super::*;
use crate::vfs::EntryKind;

fn entry(name: &str, kind: EntryKind, size: u64) -> VfsEntry {
    VfsEntry {
        name: name.to_string(),
        kind,
        size: Some(size),
        permissions: None,
        owner: None,
        group: None,
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: false,
        depth: 0,
    }
}

fn app() -> App {
    let mut app = App::new(Config::default());
    let p = app.active_panel_mut();
    p.entries = vec![
        entry("..", EntryKind::Dir, 0),
        entry("a.rs", EntryKind::File, 100),
        entry("b.rs", EntryKind::File, 200),
        entry("B.RS", EntryKind::File, 50),
        entry("notes.txt", EntryKind::File, 10),
        entry("src.rs", EntryKind::Dir, 4096),
    ];
    p.cursor = 0;
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

fn marked(app: &App) -> Vec<String> {
    let mut v: Vec<String> = app.active_panel().marked.iter().cloned().collect();
    v.sort();
    v
}

#[test]
fn plus_in_empty_cmdline_opens_select_with_star() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('+')));
    match &app.dialog {
        Some(Dialog::Input { prompt, input, op: PendingOp::Select(true) }) => {
            assert_eq!(prompt, "select");
            assert_eq!(input.text(), "*");
        }
        _ => panic!("select dialog expected"),
    }
    assert!(app.cmdline.chars.is_empty());
}

#[test]
fn minus_in_empty_cmdline_opens_unselect() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('-')));
    assert!(matches!(
        app.dialog,
        Some(Dialog::Input { op: PendingOp::Select(false), .. })
    ));
}

#[test]
fn plus_minus_typed_when_cmdline_not_empty() {
    let mut app = app();
    type_str(&mut app, "ls -la +x");
    assert!(app.dialog.is_none());
    assert_eq!(app.cmdline.text(), "ls -la +x");
}

#[test]
fn select_by_mask_marks_files_and_dirs_case_sensitive() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('+')));
    app.handle_key(ctrl('u'));
    type_str(&mut app, "*.rs");
    app.handle_key(ctrl('y'));
    assert!(app.dialog.is_none());
    // B.RS — другой регистр, `..` не помечается, директория src.rs — помечается.
    assert_eq!(marked(&app), vec!["a.rs", "b.rs", "src.rs"]);
}

#[test]
fn star_never_marks_dotdot() {
    let mut app = app();
    app.handle_key(key(KeyCode::Char('+')));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(marked(&app).len(), 5);
    assert!(!app.active_panel().marked.contains(".."));
}

#[test]
fn unselect_removes_only_matching() {
    let mut app = app();
    app.apply_mask("*", true);
    app.handle_key(key(KeyCode::Char('-')));
    app.handle_key(ctrl('u'));
    type_str(&mut app, "?.rs");
    app.handle_key(ctrl('y'));
    assert_eq!(marked(&app), vec!["B.RS", "notes.txt", "src.rs"]);
}

#[test]
fn last_mask_remembered_as_default() {
    let mut app = app();
    app.apply_mask("*.txt", true);
    app.handle_key(key(KeyCode::Char('-')));
    match &app.dialog {
        Some(Dialog::Input { input, .. }) => assert_eq!(input.text(), "*.txt"),
        _ => panic!("dialog expected"),
    }
}

#[test]
fn cancel_does_not_change_marks_or_mask() {
    for cancel in [ctrl('n'), key(KeyCode::Esc)] {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('+')));
        app.handle_key(ctrl('u'));
        type_str(&mut app, "*.rs");
        app.handle_key(cancel);
        assert!(app.dialog.is_none());
        assert!(app.active_panel().marked.is_empty());
        assert!(app.last_mask.is_none());
    }
}

#[test]
fn empty_mask_is_error() {
    let mut app = app();
    app.apply_mask("  ", true);
    assert!(app.active_panel().marked.is_empty());
    assert_eq!(app.status, "empty mask");
}

#[test]
fn summary_counts_marked_and_sums_file_sizes_only() {
    let mut app = app();
    assert_eq!(marked_summary(app.active_panel()), (0, 0));
    app.apply_mask("*.rs", true);
    // a.rs 100 + b.rs 200; директория src.rs считается, но без размера.
    assert_eq!(marked_summary(app.active_panel()), (3, 300));
}
