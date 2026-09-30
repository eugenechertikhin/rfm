//! Юнит-тесты модуля `ui` (в отдельном файле — по уставу).

use super::panel::{file_info_line, format_entry, format_mode, human_size};
use super::*;

fn mk(name: &str, kind: crate::vfs::EntryKind, mode: Option<u32>) -> VfsEntry {
    VfsEntry {
        name: name.to_string(),
        kind,
        size: Some(1024),
        permissions: mode,
        owner: Some("root".to_string()),
        group: Some("wheel".to_string()),
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: false,
        depth: 0,
    }
}

#[test]
fn mode_basic_and_type() {
    use crate::vfs::EntryKind::*;
    assert_eq!(format_mode(&mk("f", File, Some(0o644)), Some(0o644)), "-rw-r--r--");
    assert_eq!(format_mode(&mk("d", Dir, Some(0o755)), Some(0o755)), "drwxr-xr-x");
    assert_eq!(format_mode(&mk("l", Symlink, Some(0o777)), Some(0o777)), "lrwxrwxrwx");
    // Без прав — прочерки, но тип сохраняется.
    assert_eq!(format_mode(&mk("f", File, None), None), "----------");
}

#[test]
fn mode_special_bits() {
    use crate::vfs::EntryKind::File;
    // setuid с x → 's', без x → 'S'.
    assert_eq!(format_mode(&mk("f", File, Some(0o4755)), Some(0o4755)), "-rwsr-xr-x");
    assert_eq!(format_mode(&mk("f", File, Some(0o4655)), Some(0o4655)), "-rwSr-xr-x");
    // setgid.
    assert_eq!(format_mode(&mk("f", File, Some(0o2755)), Some(0o2755)), "-rwxr-sr-x");
    // sticky в триаде остальных → 't'/'T'.
    assert_eq!(format_mode(&mk("d", crate::vfs::EntryKind::Dir, Some(0o1777)), Some(0o1777)), "drwxrwxrwt");
    assert_eq!(format_mode(&mk("d", crate::vfs::EntryKind::Dir, Some(0o1666)), Some(0o1666)), "drw-rw-rwT");
}

#[test]
fn info_line_contains_all_fields() {
    let e = mk("file.txt", crate::vfs::EntryKind::File, Some(0o644));
    let line = file_info_line(&e, 60);
    assert_eq!(UnicodeWidthStr::width(line.as_str()), 60);
    assert!(line.contains("file.txt"));
    assert!(line.contains("1.0K"));
    assert!(line.contains("root:wheel"));
    assert!(line.contains("-rw-r--r--"));
}

#[test]
fn truncate_respects_width() {
    assert_eq!(truncate_width("hello", 3), "hel");
    assert_eq!(truncate_width("hello", 10), "hello");
}

#[test]
fn truncate_wide_chars() {
    // Каждый CJK-символ шириной 2.
    assert_eq!(truncate_width("日本語", 4), "日本");
    assert_eq!(truncate_width("日本語", 5), "日本");
}

#[test]
fn human_size_units() {
    assert_eq!(human_size(512), "512B");
    assert_eq!(human_size(1024), "1.0K");
    assert_eq!(human_size(1536), "1.5K");
}

#[test]
fn format_entry_fits_width() {
    let e = VfsEntry {
        name: "file.txt".to_string(),
        kind: crate::vfs::EntryKind::File,
        size: Some(2048),
        permissions: None,
        owner: None,
        group: None,
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: false,
        depth: 0,
    };
    let line = format_entry(&e, 20);
    assert_eq!(UnicodeWidthStr::width(line.as_str()), 20);
    assert!(line.starts_with("file.txt"));
    assert!(line.trim_end().ends_with("2.0K"));
}

#[test]
fn file_op_dialog_title_in_brackets() {
    use super::overlay::input_dialog_title;
    assert_eq!(input_dialog_title("Copy 2 item(s) to:", true), "[ Copy 2 item(s) to: ]");
    // Прочие диалоги (напр. пароль FTP) — без скобок.
    assert_eq!(input_dialog_title("Password:", false), "Password:");
}

#[test]
fn file_op_dialog_wider_by_percent() {
    use super::overlay::input_dialog_width;
    assert_eq!(input_dialog_width(40, 100, 200), 44);
    assert_eq!(input_dialog_width(40, 115, 200), 50); // copy/move: 40*1.15 = 46, +4
    assert_eq!(input_dialog_width(40, 150, 200), 64); // create: 40*1.5 = 60, +4
    // Минимум 20 и ограничение шириной экрана сохраняются.
    assert_eq!(input_dialog_width(5, 115, 200), 20);
    assert_eq!(input_dialog_width(100, 150, 80), 78);
}

#[test]
fn marked_title_shows_count_and_size() {
    use super::panel::marked_title;
    let mut app = crate::app::App::new(crate::config::Config::default());
    let p = app.active_panel_mut();
    p.entries = vec![
        mk("a", crate::vfs::EntryKind::File, None),
        mk("b", crate::vfs::EntryKind::File, None),
    ];
    assert_eq!(marked_title(p), None);
    p.marked.insert("a".to_string());
    assert_eq!(marked_title(p).as_deref(), Some("[ 1 file, 1.0K ]"));
    p.marked.insert("b".to_string());
    assert_eq!(marked_title(p).as_deref(), Some("[ 2 files, 2.0K ]"));
}

#[test]
fn marked_summary_drawn_on_separator_left_with_indent() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut app = crate::app::App::new(crate::config::Config::default());
    app.config.show_clock = false;
    {
        let p = app.active_panel_mut();
        p.entries = vec![mk("a", crate::vfs::EntryKind::File, None)];
        p.cursor = 0;
        p.marked.insert("a".to_string());
    }
    let mut term = Terminal::new(TestBackend::new(40, 12)).unwrap();
    term.draw(|f| render(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    let rows: Vec<String> = (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect();
    let sep = rows
        .iter()
        .find(|r| r.starts_with('├'))
        .expect("separator row");
    assert!(sep.starts_with("├─[ 1 file, 1.0K ]─"), "separator was {sep:?}");
    // На верхней рамке сводки нет.
    assert!(!rows[0].contains("file"), "top border was {:?}", rows[0]);
}

#[test]
fn top_border_title_and_clock_indented_by_one() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut app = crate::app::App::new(crate::config::Config::default());
    app.config.show_clock = true;
    let mut term = Terminal::new(TestBackend::new(60, 10)).unwrap();
    term.draw(|f| render(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    let top: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    assert!(top.starts_with("┌─[ "), "top border was {top:?}");
    let clock = format!("[ {} ]─┐", crate::clock::hh_mm());
    assert!(top.ends_with(&clock), "top border was {top:?}");
}

#[test]
fn info_line_symlink_shows_target() {
    let mut e = mk("lnk", crate::vfs::EntryKind::Symlink, Some(0o777));
    e.symlink_target = Some("../real/file".to_string());
    let line = file_info_line(&e, 60);
    assert_eq!(UnicodeWidthStr::width(line.as_str()), 60);
    assert!(line.starts_with("lnk -> ../real/file "), "line was {line:?}");
    assert!(line.ends_with("lrwxrwxrwx"));
    // Цель неизвестна — просто имя.
    e.symlink_target = None;
    assert!(file_info_line(&e, 60).starts_with("lnk "));
    // Не симлинк с заполненным полем — стрелки нет.
    let mut f = mk("f", crate::vfs::EntryKind::File, Some(0o644));
    f.symlink_target = Some("x".to_string());
    assert!(!file_info_line(&f, 60).contains("->"));
}

#[test]
fn info_line_symlink_truncated_keeps_metadata() {
    let mut e = mk("lnk", crate::vfs::EntryKind::Symlink, Some(0o777));
    e.symlink_target = Some("/very/long/target/path/that/does/not/fit".to_string());
    let line = file_info_line(&e, 40);
    assert_eq!(UnicodeWidthStr::width(line.as_str()), 40);
    assert!(line.starts_with("lnk -> /ve "), "line was {line:?}"); // 40 - 28 (метаданные) - 2
    assert!(line.ends_with("root:wheel  lrwxrwxrwx"));
}

#[test]
fn settings_two_columns_layout() {
    use super::overlay::settings_positions;
    let mut app = crate::app::App::new(crate::config::Config::default());
    let p = crate::app::Panel::new(app.panels[0].path.clone());
    app.panels.push(p);
    let rows = app.settings_rows();
    let left = SETTINGS_LEFT.len();
    let (pos, h) = settings_positions(rows.len(), true);
    assert_eq!(pos[0], (0, 0)); // Global
    assert_eq!(pos[left], (1, 0)); // Panel 1 — вверху правой колонки
    assert_eq!(pos[left + 4], (1, 4)); // Panel 2
    // Кнопки — под обеими колонками через пустую строку.
    assert_eq!(*pos.last().unwrap(), (0, left as u16 + 1));
    assert_eq!(h, left as u16 + 2);
    // Одна колонка — всё подряд.
    let (pos1, h1) = settings_positions(rows.len(), false);
    assert_eq!(pos1[left], (0, left as u16));
    assert_eq!(h1, rows.len() as u16);
}

#[test]
fn settings_render_shows_panels_in_right_column() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut app = crate::app::App::new(crate::config::Config::default());
    app.config.show_clock = false;
    let p = crate::app::Panel::new(app.panels[0].path.clone());
    app.panels.push(p);
    app.active = 1;
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.settings.is_some());
    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| render(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    let rows: Vec<String> = (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect();
    let line = rows.iter().find(|r| r.contains("── Global ──")).expect("Global header");
    assert!(line.contains("── Panel 1 ──"), "row was {line:?}"); // правая колонка на той же строке
    assert!(rows.iter().any(|r| r.contains("── Panel 2 (active) ──")));
    assert!(!rows.iter().any(|r| r.contains("Current panel")));
    assert!(rows.iter().any(|r| r.contains("[ Save (c-s) ]  [ Cancel (c-n) ]")));
}

#[test]
fn dialog_buttons_in_brackets() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{backend::TestBackend, Terminal};
    let screen = |app: &mut crate::app::App| -> String {
        let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
        term.draw(|f| render(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut app = crate::app::App::new(crate::config::Config::default());
    app.active_panel_mut().entries = vec![mk("a", crate::vfs::EntryKind::File, None)];
    app.active_panel_mut().cursor = 0;
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    let s = screen(&mut app);
    assert!(s.contains("[ Delete (c-y) ]") && s.contains("[ Sudo (c-s) ]") && s.contains("[ Cancel (c-n) ]"));

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE));
    let s = screen(&mut app);
    assert!(s.contains("[ OK (c-y) ]") && s.contains("[ Cancel (c-n) ]"));
}


#[test]
fn editor_quit_confirm_has_yes_cancel_buttons() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut app = crate::app::App::new(crate::config::Config::default());
    app.dialog = Some(crate::app::Dialog::Confirm {
        message: "File has been modified. Quit without saving?".to_string(),
        op: crate::app::PendingOp::QuitEditor,
    });
    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| render(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    let rows: Vec<String> = (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect();
    assert!(rows.iter().any(|r| r.contains("[ Confirm ]")));
    assert!(rows.iter().any(|r| r.contains("File has been modified. Quit without saving?")));
    assert!(rows.iter().any(|r| r.contains("[ Yes (c-y) ]  [ Cancel (c-n) ]")));
    assert!(!rows.iter().any(|r| r.contains("Sudo")));
}

#[test]
fn focused_dialog_button_uses_button_sel_bg() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut app = crate::app::App::new(crate::config::Config::default());
    app.dialog = Some(crate::app::Dialog::Confirm {
        message: "Delete a?".to_string(),
        op: crate::app::PendingOp::Delete(vec![]),
    });
    app.dialog_btn = 1; // Sudo
    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| render(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    let theme = app.theme.clone();
    // Находим первую ячейку подписи кнопки по тексту строки.
    let cell_of = |label: &str| {
        (0..buf.area.height)
            .find_map(|y| {
                let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
                row.find(label).map(|i| (row[..i].chars().count() as u16, y))
            })
            .unwrap()
    };
    let (x, y) = cell_of("[ Sudo (c-s) ]");
    assert_eq!(buf[(x, y)].bg, theme.button_sel_bg);
    assert_eq!(buf[(x, y)].fg, theme.button_sel_fg);
    let (x, y) = cell_of("[ Delete (c-y) ]");
    assert_eq!(buf[(x, y)].bg, theme.cursor_bg); // не в фокусе — цвет курсора
}
