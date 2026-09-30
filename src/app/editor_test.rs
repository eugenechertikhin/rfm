//! Юнит-тесты встроенного редактора (в отдельном файле — по уставу).

use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Уникальная временная директория (без внешних крейтов).
fn tmp_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rfm_edit_test_{}_{}", std::process::id(), n));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn editor_with(content: &str) -> (Editor, PathBuf) {
    let dir = tmp_dir();
    let path = dir.join("f.txt");
    fs::write(&path, content).unwrap();
    (Editor::load(&path).unwrap(), dir)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn app_with_editor(content: &str) -> (App, PathBuf) {
    let (ed, dir) = editor_with(content);
    let mut app = App::new(crate::config::Config::default());
    app.active_panel_mut().editor = Some(ed);
    (app, dir)
}

fn file_text(dir: &PathBuf) -> String {
    fs::read_to_string(dir.join("f.txt")).unwrap()
}

// ---- Загрузка/сохранение ----

#[test]
fn load_save_roundtrip_keeps_trailing_newline() {
    let (ed, dir) = editor_with("one\ntwo\n");
    assert_eq!(ed.lines, vec!["one", "two"]);
    ed.save().unwrap();
    assert_eq!(file_text(&dir), "one\ntwo\n");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_save_roundtrip_no_trailing_newline() {
    let (ed, dir) = editor_with("one\ntwo");
    assert_eq!(ed.lines, vec!["one", "two"]);
    ed.save().unwrap();
    assert_eq!(file_text(&dir), "one\ntwo");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_empty_file_gives_one_empty_line() {
    let (ed, dir) = editor_with("");
    assert_eq!(ed.lines, vec![""]);
    fs::remove_dir_all(&dir).ok();
}

// ---- Движение курсора ----

#[test]
fn right_at_eol_wraps_to_next_line() {
    let (mut ed, dir) = editor_with("ab\ncd\n");
    ed.move_right();
    ed.move_right();
    assert_eq!((ed.cur_line, ed.cur_col), (0, 2));
    ed.move_right(); // конец строки → начало следующей
    assert_eq!((ed.cur_line, ed.cur_col), (1, 0));
    ed.move_right();
    assert_eq!((ed.cur_line, ed.cur_col), (1, 1));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn left_at_start_wraps_to_prev_line_end() {
    let (mut ed, dir) = editor_with("ab\ncd\n");
    ed.cur_line = 1;
    ed.cur_col = 0;
    ed.move_left(); // начало строки → конец предыдущей
    assert_eq!((ed.cur_line, ed.cur_col), (0, 2));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn right_at_end_of_file_stays() {
    let (mut ed, dir) = editor_with("a\n");
    ed.move_right();
    ed.move_right();
    assert_eq!((ed.cur_line, ed.cur_col), (0, 1));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn up_down_clamp_column() {
    let (mut ed, dir) = editor_with("long line\nab\n");
    ed.cur_col = 7;
    ed.move_down(1); // "ab" короче — колонка прижимается
    assert_eq!((ed.cur_line, ed.cur_col), (1, 2));
    ed.move_up(1);
    assert_eq!((ed.cur_line, ed.cur_col), (0, 2));
    fs::remove_dir_all(&dir).ok();
}

// ---- Правка ----

#[test]
fn insert_char_multibyte_safe() {
    let (mut ed, dir) = editor_with("мир\n");
    ed.cur_col = 1;
    ed.insert_char('ё');
    assert_eq!(ed.lines[0], "мёир");
    assert_eq!(ed.cur_col, 2);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn enter_splits_line() {
    let (mut ed, dir) = editor_with("abcd\n");
    ed.cur_col = 2;
    ed.insert_newline();
    assert_eq!(ed.lines, vec!["ab", "cd"]);
    assert_eq!((ed.cur_line, ed.cur_col), (1, 0));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn backspace_deletes_and_joins() {
    let (mut ed, dir) = editor_with("ab\ncd\n");
    ed.cur_line = 1;
    ed.cur_col = 1;
    assert!(ed.backspace()); // удалили 'c'
    assert_eq!(ed.lines[1], "d");
    assert!(ed.backspace()); // начало строки — склейка с предыдущей
    assert_eq!(ed.lines, vec!["abd"]);
    assert_eq!((ed.cur_line, ed.cur_col), (0, 2));
    ed.cur_col = 0;
    assert!(!ed.backspace()); // начало файла — удалять нечего
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn delete_removes_and_joins() {
    let (mut ed, dir) = editor_with("ab\ncd\n");
    ed.cur_col = 2;
    assert!(ed.delete()); // конец строки — склейка со следующей
    assert_eq!(ed.lines, vec!["abcd"]);
    assert!(ed.delete()); // удалили символ под курсором ('c')
    assert_eq!(ed.lines, vec!["abd"]);
    fs::remove_dir_all(&dir).ok();
}

// ---- Прокрутка и восстановление ----

#[test]
fn ensure_visible_scrolls_viewport() {
    let (mut ed, dir) = editor_with(&"x\n".repeat(50));
    ed.cur_line = 30;
    ed.ensure_visible(10, 10);
    assert!(ed.voff <= 30 && 30 < ed.voff + 10);
    ed.cur_line = 0;
    ed.ensure_visible(10, 10);
    assert_eq!(ed.voff, 0);
    ed.cur_col = 25;
    ed.ensure_visible(10, 10);
    assert!(ed.hoff <= 25 && 25 < ed.hoff + 10);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn restore_cursor_clamps_to_content() {
    let (mut ed, dir) = editor_with("ab\ncd\n");
    ed.restore_cursor(1, 1);
    assert_eq!((ed.cur_line, ed.cur_col), (1, 1));
    // Позиция за пределами (файл «уменьшился») — клампится.
    ed.restore_cursor(99, 99);
    assert_eq!((ed.cur_line, ed.cur_col), (1, 2));
    fs::remove_dir_all(&dir).ok();
}

// ---- Клавиши через App: правки в буфере, на диск — только save ----

#[test]
fn typing_edits_buffer_not_disk() {
    let (mut app, dir) = app_with_editor("ab\n");
    app.handle_key(key(KeyCode::Char('X')));
    app.handle_key(key(KeyCode::Enter));
    // Буфер изменился и помечен dirty, файл на диске не тронут.
    let ed = app.active_panel().editor.as_ref().unwrap();
    assert_eq!(ed.lines, vec!["X", "ab"]);
    assert!(ed.dirty);
    assert_eq!(file_text(&dir), "ab\n");
    fs::remove_dir_all(&dir).ok();
}

fn quit_dialog_open(app: &App) -> bool {
    matches!(app.dialog, Some(Dialog::Confirm { op: PendingOp::QuitEditor, .. }))
}

#[test]
fn esc_unmodified_closes_without_dialog() {
    let (mut app, dir) = app_with_editor("ab\n");
    app.handle_key(key(KeyCode::Esc));
    assert!(app.active_panel().editor.is_none());
    assert!(app.dialog.is_none());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn esc_modified_asks_then_yes_closes_without_saving() {
    for yes in [ctrl('y'), key(KeyCode::Enter)] {
        let (mut app, dir) = app_with_editor("ab\n");
        app.handle_key(key(KeyCode::Char('X')));
        app.handle_key(key(KeyCode::Esc));
        assert!(quit_dialog_open(&app));
        assert!(app.active_panel().editor.is_some()); // ещё открыт
        app.handle_key(yes);
        assert!(app.dialog.is_none());
        assert!(app.active_panel().editor.is_none());
        assert_eq!(file_text(&dir), "ab\n"); // несохранённое отброшено
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn quit_dialog_cancel_stays_in_editor_with_edits() {
    for cancel in [ctrl('n'), key(KeyCode::Esc)] {
        let (mut app, dir) = app_with_editor("ab\n");
        app.handle_key(key(KeyCode::Char('X')));
        app.handle_key(key(KeyCode::Esc));
        app.handle_key(cancel);
        assert!(app.dialog.is_none());
        let ed = app.active_panel().editor.as_ref().expect("editor stays open");
        assert_eq!(ed.lines, vec!["Xab"]);
        assert!(ed.dirty);
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn quit_dialog_ignores_other_keys() {
    let (mut app, dir) = app_with_editor("ab\n");
    app.handle_key(key(KeyCode::Char('X')));
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Char('z')));
    app.handle_key(ctrl('s')); // sudo — только для delete
    assert!(quit_dialog_open(&app));
    assert_eq!(app.active_panel().editor.as_ref().unwrap().lines, vec!["Xab"]);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_x_q_modified_asks_same_dialog() {
    let (mut app, dir) = app_with_editor("ab\n");
    app.handle_key(key(KeyCode::Char('X')));
    app.handle_key(ctrl('x'));
    assert_eq!(app.prefix, Prefix::Root);
    app.handle_key(key(KeyCode::Char('q')));
    assert_eq!(app.prefix, Prefix::None);
    assert!(quit_dialog_open(&app));
    app.handle_key(ctrl('y'));
    assert!(app.active_panel().editor.is_none());
    assert_eq!(file_text(&dir), "ab\n");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_x_q_unmodified_closes_without_dialog() {
    let (mut app, dir) = app_with_editor("ab\n");
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('q')));
    assert!(app.dialog.is_none());
    assert!(app.active_panel().editor.is_none());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_x_s_saves_and_stays() {
    let (mut app, dir) = app_with_editor("ab\n");
    app.handle_key(key(KeyCode::Char('Z')));
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('s')));
    assert!(app.active_panel().editor.is_some()); // остаёмся в редакторе
    assert_eq!(app.status, "saved");
    assert_eq!(file_text(&dir), "Zab\n");
    assert!(!app.active_panel().editor.as_ref().unwrap().dirty); // dirty снят
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_x_x_saves_and_closes() {
    let (mut app, dir) = app_with_editor("ab\n");
    app.handle_key(key(KeyCode::Char('Z')));
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.active_panel().editor.is_none());
    assert_eq!(file_text(&dir), "Zab\n");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ctrl_keys_do_not_insert() {
    let (mut app, dir) = app_with_editor("ab\n");
    app.handle_key(ctrl('a'));
    app.handle_key(ctrl('q')); // в редакторе не выход из приложения и не текст
    assert_eq!(file_text(&dir), "ab\n");
    assert!(app.active_panel().editor.is_some());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn tab_inserts_four_spaces() {
    let (mut app, dir) = app_with_editor("x\n");
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('s')));
    assert_eq!(file_text(&dir), "    x\n");
    fs::remove_dir_all(&dir).ok();
}

// ---- Мышь ----

fn click(col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn click_places_cursor() {
    let (mut app, dir) = app_with_editor("hello\nworld\n");
    app.active_panel_mut().area = Rect::new(0, 0, 20, 10);
    // Рамка 1px: экранная точка (3, 2) → строка 1, колонка 2.
    app.handle_mouse(click(3, 2));
    let ed = app.active_panel().editor.as_ref().unwrap();
    assert_eq!((ed.cur_line, ed.cur_col), (1, 2));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn click_clamps_to_text() {
    let (mut app, dir) = app_with_editor("ab\ncd\n");
    app.active_panel_mut().area = Rect::new(0, 0, 20, 10);
    // Клик далеко за концом строки и ниже последней строки.
    app.handle_mouse(click(15, 8));
    let ed = app.active_panel().editor.as_ref().unwrap();
    assert_eq!((ed.cur_line, ed.cur_col), (1, 2)); // последняя строка, её конец
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn click_respects_scroll_offsets() {
    let (mut app, dir) = app_with_editor(&"x\n".repeat(50));
    app.active_panel_mut().area = Rect::new(0, 0, 20, 10);
    if let Some(ed) = app.active_panel_mut().editor.as_mut() {
        ed.voff = 20;
    }
    app.handle_mouse(click(1, 1)); // верхний левый угол текста
    let ed = app.active_panel().editor.as_ref().unwrap();
    assert_eq!(ed.cur_line, 20);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn click_wide_chars_maps_display_column() {
    // CJK-символы занимают 2 ячейки: клик в ячейку 4 — это третий символ (индекс 2).
    let (mut app, dir) = app_with_editor("日本語abc\n");
    app.active_panel_mut().area = Rect::new(0, 0, 20, 10);
    app.handle_mouse(click(5, 1)); // rel_col = 4
    let ed = app.active_panel().editor.as_ref().unwrap();
    assert_eq!(ed.cur_col, 2);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn click_on_border_only_activates() {
    let (mut app, dir) = app_with_editor("ab\ncd\n");
    app.active_panel_mut().area = Rect::new(0, 0, 20, 10);
    if let Some(ed) = app.active_panel_mut().editor.as_mut() {
        ed.cur_line = 1;
        ed.cur_col = 1;
    }
    app.handle_mouse(click(0, 0)); // рамка — курсор не двигается
    let ed = app.active_panel().editor.as_ref().unwrap();
    assert_eq!((ed.cur_line, ed.cur_col), (1, 1));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn internal_edit_opens_editor_not_viewer() {
    let dir = tmp_dir();
    let path = dir.join("g.txt");
    fs::write(&path, "hello\n").unwrap();
    let mut app = App::new(crate::config::Config::default());
    app.active_panel_mut().path = VfsPath::local(dir.clone());
    app.reload();
    let idx = app
        .active_panel()
        .entries
        .iter()
        .position(|e| e.name == "g.txt")
        .unwrap();
    app.active_panel_mut().cursor = idx;
    app.op_edit();
    assert!(app.active_panel().editor.is_some());
    assert!(app.active_panel().viewer.is_none());
    fs::remove_dir_all(&dir).ok();
}
