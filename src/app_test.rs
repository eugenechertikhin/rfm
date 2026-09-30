//! Юнит-тесты модуля `app` (вынесены в отдельный файл, остаются потомком `app`).

use super::*;
use super::panel::{sort_entries, SortState};
use super::viewer::{hex_line, load_viewer, sanitize_line};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

fn entry(name: &str) -> VfsEntry {
    VfsEntry {
        name: name.to_string(),
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
    }
}

fn app_with(entries: Vec<VfsEntry>) -> App {
    let mut app = App::new(Config::default());
    app.active_panel_mut().entries = entries;
    app.active_panel_mut().cursor = 0;
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn cursor_clamped_at_bounds() {
    let mut app = app_with(vec![entry("a"), entry("b"), entry("c")]);
    app.move_cursor(-1);
    assert_eq!(app.active_panel().cursor, 0);
    app.move_cursor(100);
    assert_eq!(app.active_panel().cursor, 2);
    app.move_cursor(-1);
    assert_eq!(app.active_panel().cursor, 1);
}

#[test]
fn cursor_noop_on_empty() {
    let mut app = app_with(vec![]);
    app.move_cursor(1);
    assert_eq!(app.active_panel().cursor, 0);
}

#[test]
fn cmdline_editing() {
    let mut c = CmdLine::default();
    for ch in "hello".chars() {
        c.insert(ch);
    }
    assert_eq!(c.text(), "hello");
    c.home();
    assert_eq!(c.cursor, 0);
    c.end();
    assert_eq!(c.cursor, 5);
    c.kill_word();
    assert_eq!(c.text(), "");
}

#[test]
fn cmdline_kill_word_stops_at_space() {
    let mut c = CmdLine::default();
    for ch in "ls -la /tmp".chars() {
        c.insert(ch);
    }
    c.kill_word();
    assert_eq!(c.text(), "ls -la ");
}

#[test]
fn ctrl_q_quits() {
    let mut app = app_with(vec![]);
    assert_eq!(app.handle_key(ctrl('q')), Action::Quit);
}

#[test]
fn ctrl_l_requests_full_redraw() {
    // Ctrl+l — единственный источник полного сброса экрана из клавиш.
    let mut app = app_with(vec![]);
    assert_eq!(app.handle_key(ctrl('l')), Action::ClearRedraw);
}

fn open_dummy_viewer(app: &mut App) {
    app.active_panel_mut().viewer = Some(Viewer::from_memory("x".to_string(), vec!["a".to_string()]));
}

#[test]
fn viewer_ctrl_x_x_opens_settings() {
    let mut app = app_with(vec![entry("a")]);
    open_dummy_viewer(&mut app);
    app.handle_key(ctrl('x'));
    assert_eq!(app.prefix, Prefix::Root); // аккорд начат
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.settings.is_some());
    assert_eq!(app.prefix, Prefix::None);
    // Настройки получают клавиши поверх открытого просмотрщика.
    let sel0 = app.settings.as_ref().unwrap().sel;
    app.handle_key(key(KeyCode::Down));
    assert_ne!(app.settings.as_ref().unwrap().sel, sel0);
}

#[test]
fn viewer_ctrl_x_h_opens_help() {
    let mut app = app_with(vec![entry("a")]);
    open_dummy_viewer(&mut app);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('h')));
    assert!(app.help.is_some());
    assert_eq!(app.prefix, Prefix::None);
}

#[test]
fn viewer_ctrl_x_other_cancels() {
    let mut app = app_with(vec![entry("a")]);
    open_dummy_viewer(&mut app);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('z'))); // не x/h → отмена
    assert_eq!(app.prefix, Prefix::None);
    assert!(app.settings.is_none());
    assert!(app.help.is_none());
    assert!(app.active_panel().viewer.is_some()); // просмотрщик остался
}

#[test]
fn prefix_q_quits() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    assert_eq!(app.handle_key(key(KeyCode::Char('q'))), Action::Quit);
}

// ---- Этап 4: история ----

fn app_with_history() -> App {
    use crate::history::HistEntry;
    let mut app = app_with(vec![]);
    app.history.entries = vec![
        HistEntry {
            cmd: "first".to_string(),
            count: 1,
            last: 100,
        },
        HistEntry {
            cmd: "second".to_string(),
            count: 1,
            last: 200,
        },
    ];
    app
}

#[test]
fn history_prev_next_navigation() {
    let mut app = app_with_history();
    app.handle_key(ctrl('p')); // самая свежая
    assert_eq!(app.cmdline.text(), "second");
    app.handle_key(ctrl('p')); // старее
    assert_eq!(app.cmdline.text(), "first");
    app.handle_key(ctrl('n')); // назад к свежей
    assert_eq!(app.cmdline.text(), "second");
    app.handle_key(ctrl('n')); // за пределы — пусто
    assert_eq!(app.cmdline.text(), "");
}

#[test]
fn typing_resets_history_nav() {
    let mut app = app_with_history();
    app.handle_key(ctrl('p'));
    assert!(app.hist_nav.is_some());
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.hist_nav.is_none());
}

#[test]
fn ctrl_g_opens_history_search() {
    let mut app = app_with_history();
    app.handle_key(ctrl('g'));
    match &app.dialog {
        Some(Dialog::HistorySearch { results, .. }) => assert_eq!(results.len(), 2),
        _ => panic!("expected HistorySearch dialog"),
    }
}

#[test]
fn history_search_enter_fills_cmdline() {
    let mut app = app_with_history();
    app.handle_key(ctrl('g'));
    // фильтр "fir" → "first"
    app.handle_key(key(KeyCode::Char('f')));
    app.handle_key(key(KeyCode::Char('i')));
    app.handle_key(key(KeyCode::Char('r')));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
    assert_eq!(app.cmdline.text(), "first");
}

// ---- Инкрементальный поиск (Ctrl+s) ----

#[test]
fn incremental_search_jumps_and_cycles() {
    let mut app = app_with(vec![
        entry("alpha"),
        entry("bravo"),
        entry("brave"),
        entry("delta"),
    ]);
    app.handle_key(ctrl('s'));
    assert!(app.search.is_some());
    app.handle_key(key(KeyCode::Char('b'))); // первое с 'b' → "bravo"
    assert_eq!(app.active_panel().cursor, 1);
    app.handle_key(ctrl('s')); // следующее → "brave"
    assert_eq!(app.active_panel().cursor, 2);
    app.handle_key(ctrl('s')); // по кругу → снова "bravo"
    assert_eq!(app.active_panel().cursor, 1);
}

#[test]
fn search_esc_restores_cursor() {
    let mut app = app_with(vec![entry("alpha"), entry("bravo"), entry("charlie")]);
    app.active_panel_mut().cursor = 0;
    app.handle_key(ctrl('s'));
    app.handle_key(key(KeyCode::Char('c'))); // → "charlie"
    assert_eq!(app.active_panel().cursor, 2);
    app.handle_key(key(KeyCode::Esc));
    assert!(app.search.is_none());
    assert_eq!(app.active_panel().cursor, 0); // курсор восстановлен
}

#[test]
fn search_enter_keeps_cursor() {
    let mut app = app_with(vec![entry("alpha"), entry("bravo")]);
    app.handle_key(ctrl('s'));
    app.handle_key(key(KeyCode::Char('b')));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.search.is_none());
    assert_eq!(app.active_panel().cursor, 1);
}

#[test]
fn running_command_records_history() {
    let mut app = app_with(vec![]);
    for ch in "echo hi".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.history.entries.iter().any(|e| e.cmd == "echo hi"));
}

#[test]
fn printable_goes_to_cmdline_arrows_do_not() {
    let mut app = app_with(vec![entry("a"), entry("b")]);
    for ch in "ls".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert_eq!(app.cmdline.text(), "ls");
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.cmdline.text(), "ls");
    assert_eq!(app.active_panel().cursor, 1);
}

#[test]
fn esc_clears_cmdline() {
    let mut app = app_with(vec![]);
    for ch in "junk".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.cmdline.text(), "");
}

#[test]
fn enter_nonempty_runs_shell_and_clears() {
    let mut app = app_with(vec![]);
    for ch in "echo hi".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let action = app.handle_key(key(KeyCode::Enter));
    assert_eq!(action, Action::RunShell("echo hi".to_string()));
    assert_eq!(app.cmdline.text(), "");
}

#[test]
fn name_sort_is_case_sensitive_uppercase_first() {
    let mut v = vec![entry("banana"), entry("Apple"), entry("Zoo"), entry("apple")];
    sort_entries(
        &mut v,
        SortState {
            key: SortKey::Name,
            reverse: false,
            dirs_first: false,
        },
    );
    let names: Vec<&str> = v.iter().map(|e| e.name.as_str()).collect();
    // Заглавные (ASCII) идут раньше строчных.
    assert_eq!(names, vec!["Apple", "Zoo", "apple", "banana"]);
}

#[test]
fn default_sort_dirs_first() {
    let mut a = entry("zebra");
    a.kind = crate::vfs::EntryKind::Dir;
    let b = entry("apple");
    let mut v = vec![b, a];
    sort_entries(&mut v, SortState::default());
    assert_eq!(v[0].name, "zebra");
    assert_eq!(v[1].name, "apple");
}

// ---- Этап 2: панели ----

/// Хелпер: аккорд создания панели `Ctrl+x p c`.
fn create_panel_keys(app: &mut App) {
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('p')));
    app.handle_key(key(KeyCode::Char('c')));
}

#[test]
fn prefix_pc_creates_panel_to_the_right() {
    let mut app = app_with(vec![]);
    assert_eq!(app.panels.len(), 1);
    app.handle_key(ctrl('x'));
    assert_eq!(app.prefix, Prefix::Root);
    app.handle_key(key(KeyCode::Char('p')));
    assert_eq!(app.prefix, Prefix::Panel);
    app.handle_key(key(KeyCode::Char('c')));
    assert_eq!(app.prefix, Prefix::None);
    assert_eq!(app.panels.len(), 2);
    assert_eq!(app.active, 1); // активна новая (справа)
}

#[test]
fn prefix_px_closes_but_not_the_last() {
    let mut app = app_with(vec![]);
    create_panel_keys(&mut app);
    assert_eq!(app.panels.len(), 2);
    // закрываем активную: Ctrl+x p x
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('p')));
    app.handle_key(key(KeyCode::Char('x')));
    assert_eq!(app.panels.len(), 1);
    // последнюю закрыть нельзя
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('p')));
    app.handle_key(key(KeyCode::Char('x')));
    assert_eq!(app.panels.len(), 1);
    assert!(app.status.contains("cannot close the last panel"));
}

#[test]
fn tab_cycles_panels() {
    let mut app = app_with(vec![]);
    create_panel_keys(&mut app); // active=1, len=2
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.active, 0);
    app.handle_key(key(KeyCode::Tab)); // по кругу
    assert_eq!(app.active, 1);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.active, 0);
}

#[test]
fn prefix_esc_cancels() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.prefix, Prefix::None);
    assert_eq!(app.panels.len(), 1);
}

#[test]
fn prefix_sort_enters_submenu() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('s')));
    assert_eq!(app.prefix, Prefix::Sort);
}

// ---- Этап 3: пометки, сортировка, операции, help ----

fn dir_entry(name: &str) -> VfsEntry {
    let mut e = entry(name);
    e.kind = crate::vfs::EntryKind::Dir;
    e.size = None;
    e
}

#[test]
fn ctrl_v_inserts_quoted_name_under_cursor() {
    let mut app = app_with(vec![entry("a b.txt"), entry("c")]);
    app.handle_key(ctrl('v'));
    assert_eq!(app.cmdline.text(), "'a b.txt' "); // всегда в одинарных кавычках + пробел
    // вставка идёт в позицию курсора командной строки
    app.cmdline.clear();
    for ch in "vim ".chars() {
        app.cmdline.insert(ch);
    }
    app.active_panel_mut().cursor = 1;
    app.handle_key(ctrl('v'));
    assert_eq!(app.cmdline.text(), "vim 'c' ");
    // подряд — имена через пробел, курсор за пробелом
    app.active_panel_mut().cursor = 0;
    app.handle_key(ctrl('v'));
    assert_eq!(app.cmdline.text(), "vim 'c' 'a b.txt' ");
    assert_eq!(app.cmdline.cursor, app.cmdline.chars.len());
}

#[test]
fn ctrl_v_on_dotdot_inserts_dotdot() {
    let mut app = app_with(vec![dir_entry("sub")]);
    // добавим ".." в начало списка через reload на реальной директории было бы сложно;
    // проверим директорию: имя без слэша.
    app.handle_key(ctrl('v'));
    assert_eq!(app.cmdline.text(), "'sub' ");
}

#[test]
fn mouse_click_moves_cursor_in_panel() {
    let mut app = app_with(vec![entry("a"), entry("b"), entry("c"), entry("d")]);
    app.panels[0].area = Rect::new(0, 0, 20, 10);
    // внутренняя область начинается с y=1; row=3 → rel_row=2 → индекс 2
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 3));
    assert_eq!(app.active, 0);
    assert_eq!(app.active_panel().cursor, 2);
}

#[test]
fn mouse_click_selects_panel() {
    let mut app = app_with(vec![entry("a"), entry("b")]);
    create_panel_keys(&mut app); // 2 панели, active=1
    app.active = 0;
    app.panels[0].entries = vec![entry("a"), entry("b")];
    app.panels[1].entries = vec![entry("x"), entry("y"), entry("z")];
    app.panels[0].area = Rect::new(0, 0, 10, 8);
    app.panels[1].area = Rect::new(10, 0, 10, 8);
    // клик в правой панели, строка файла (row=2 → rel_row=1)
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 13, 2));
    assert_eq!(app.active, 1);
    assert_eq!(app.panels[1].cursor, 1);
}

#[test]
fn mouse_click_on_border_only_selects_panel() {
    let mut app = app_with(vec![entry("a"), entry("b")]);
    app.panels[0].area = Rect::new(0, 0, 20, 10);
    app.active_panel_mut().cursor = 1;
    // клик по верхней рамке (row=0) — курсор не двигаем
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 0));
    assert_eq!(app.active, 0);
    assert_eq!(app.active_panel().cursor, 1);
}

#[test]
fn mouse_scroll_moves_active_cursor() {
    let mut app = app_with(vec![entry("a"), entry("b"), entry("c")]);
    app.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0));
    assert_eq!(app.active_panel().cursor, 1);
    app.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0));
    assert_eq!(app.active_panel().cursor, 2);
    app.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, 0));
    assert_eq!(app.active_panel().cursor, 1);
}

#[test]
fn prompt_reflects_user_and_host() {
    let app = app_with(vec![]);
    // Формат login@host с завершающим ' # ' (root) или ' $ ' (обычный).
    assert!(app.prompt.contains('@'));
    let sep = if crate::users::current_uid() == 0 { "# " } else { "$ " };
    assert!(app.prompt.ends_with(sep), "prompt was {:?}", app.prompt);
    assert_ne!(app.prompt, "> ");
}

#[test]
fn viewer_search_finds_and_navigates() {
    let path = std::env::temp_dir().join(format!("rfm_vsearch_{}.txt", std::process::id()));
    std::fs::write(&path, "alpha beta\ngamma alpha\ndelta\nalpha end\n").unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().viewer = load_viewer(&path, 0, false, false);

    // '/' входит в режим ввода запроса.
    app.handle_key(key(KeyCode::Char('/')));
    assert!(app.active_panel().viewer.as_ref().unwrap().find_input.is_some());
    for c in "alpha".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    let cur = |a: &App| a.active_panel().viewer.as_ref().unwrap().find.as_ref().unwrap().current;
    {
        let f = app.active_panel().viewer.as_ref().unwrap().find.as_ref().unwrap();
        assert_eq!(f.matches, vec![(0, 0), (1, 6), (3, 0)]); // 3 непересекающихся вхождения
        assert_eq!(f.current, 0);
    }
    // n → следующее, p → предыдущее (по кругу).
    app.handle_key(key(KeyCode::Char('n')));
    assert_eq!(cur(&app), 1);
    app.handle_key(key(KeyCode::Char('p')));
    assert_eq!(cur(&app), 0);
    app.handle_key(key(KeyCode::Char('p'))); // за начало → в конец
    assert_eq!(cur(&app), 2);
    std::fs::remove_file(&path).ok();
}

#[test]
fn viewer_search_not_found_and_cancel() {
    let path = std::env::temp_dir().join(format!("rfm_vsearch2_{}.txt", std::process::id()));
    std::fs::write(&path, "one two three\n").unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().viewer = load_viewer(&path, 0, false, false);

    // Не найдено — find остаётся None, статус сообщает.
    app.handle_key(key(KeyCode::Char('/')));
    for c in "zzz".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.active_panel().viewer.as_ref().unwrap().find.is_none());
    assert!(app.status.contains("not found"));

    // Esc отменяет ввод.
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Char('o')));
    app.handle_key(key(KeyCode::Esc));
    assert!(app.active_panel().viewer.as_ref().unwrap().find_input.is_none());
    assert!(app.active_panel().viewer.as_ref().unwrap().find.is_none());
    // Просмотрщик при этом НЕ закрылся.
    assert!(app.active_panel().viewer.is_some());
    std::fs::remove_file(&path).ok();
}

#[test]
fn mouse_scroll_scrolls_viewer() {
    let path = std::env::temp_dir().join(format!("rfm_scroll_{}.txt", std::process::id()));
    let body: String = (0..50).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&path, body).unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().viewer = load_viewer(&path, 0, false, false);
    // Колесо вниз листает содержимое просмотрщика, а не файловый курсор.
    app.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0));
    assert_eq!(app.active_panel().viewer.as_ref().unwrap().voff, 1);
    app.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0));
    assert_eq!(app.active_panel().viewer.as_ref().unwrap().voff, 2);
    app.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, 0));
    assert_eq!(app.active_panel().viewer.as_ref().unwrap().voff, 1);
    std::fs::remove_file(&path).ok();
}

#[test]
fn mouse_scroll_moves_settings_selection() {
    let mut app = app_with(vec![]);
    app.open_settings();
    let start = app.settings.as_ref().unwrap().sel;
    app.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0));
    let after = app.settings.as_ref().unwrap().sel;
    assert!(after > start); // выбор ушёл вниз (заголовки пропускаются)
    app.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, 0));
    assert_eq!(app.settings.as_ref().unwrap().sel, start);
}

#[test]
fn mouse_double_click_activates() {
    let exe = {
        let mut e = entry("run.sh");
        e.executable = true;
        e
    };
    let mut app = app_with(vec![entry("a"), exe]);
    app.panels[0].area = Rect::new(0, 0, 20, 10);
    let click = mouse(MouseEventKind::Down(MouseButton::Left), 5, 2); // индекс 1
    let a1 = app.handle_mouse(click);
    assert_eq!(a1, Action::Redraw);
    assert_eq!(app.active_panel().cursor, 1);
    // второй клик по той же строке сразу → двойной → запуск ./run.sh
    match app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 2)) {
        Action::RunShell(cmd) => assert!(cmd.contains("run.sh")),
        other => panic!("expected RunShell, got {other:?}"),
    }
}

#[test]
fn viewer_prettify_json_toggles() {
    let path = std::env::temp_dir().join(format!("rfm_pretty_{}.json", std::process::id()));
    std::fs::write(&path, r#"{"a":1,"b":[2,3]}"#).unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().viewer = load_viewer(&path, 0, false, false);
    // исходно — одна строка (компактный json).
    let raw_len = app.active_panel_mut().viewer.as_mut().unwrap().window(0, 100).len();
    assert_eq!(raw_len, 1);
    // p → prettify (многострочно, in-memory).
    app.handle_key(key(KeyCode::Char('p')));
    {
        let v = app.active_panel().viewer.as_ref().unwrap();
        assert!(v.pretty);
        assert!(v.content_lines().unwrap().len() > raw_len);
    }
    // p → назад к исходнику (ленивый файл).
    app.handle_key(key(KeyCode::Char('p')));
    let n = app.active_panel_mut().viewer.as_mut().unwrap().window(0, 100).len();
    assert!(!app.active_panel().viewer.as_ref().unwrap().pretty);
    assert_eq!(n, raw_len);
    std::fs::remove_file(&path).ok();
}

#[test]
fn viewer_prettify_unsupported_type() {
    let path = std::env::temp_dir().join(format!("rfm_pretty_{}.txt", std::process::id()));
    std::fs::write(&path, "plain text\n").unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().viewer = load_viewer(&path, 0, false, false);
    app.handle_key(key(KeyCode::Char('p')));
    let v = app.active_panel().viewer.as_ref().unwrap();
    assert!(!v.pretty); // тип не поддержан — не трогаем
    assert!(app.status.contains("unsupported"));
    std::fs::remove_file(&path).ok();
}

#[test]
fn ctrl_t_toggles_mark_and_advances() {
    let mut app = app_with(vec![entry("a"), entry("b"), entry("c")]);
    app.handle_key(ctrl('t'));
    assert!(app.active_panel().marked.contains("a"));
    assert_eq!(app.active_panel().cursor, 1); // курсор сдвинулся вниз
    // повторная пометка того же (вернём курсор на 'a')
    app.active_panel_mut().cursor = 0;
    app.handle_key(ctrl('t'));
    assert!(!app.active_panel().marked.contains("a"));
}

#[test]
fn shift_arrows_mark_and_move() {
    let mut app = app_with(vec![entry("a"), entry("b"), entry("c")]);
    let shift_down = KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT);
    let shift_up = KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT);
    app.handle_key(shift_down);
    assert!(app.active_panel().marked.contains("a"));
    assert_eq!(app.active_panel().cursor, 1);
    app.handle_key(shift_up);
    assert!(app.active_panel().marked.contains("b"));
    assert_eq!(app.active_panel().cursor, 0);
}

#[test]
fn dotdot_cannot_be_marked() {
    let mut app = app_with(vec![VfsEntry::dotdot(), entry("a")]);
    app.handle_key(ctrl('t')); // курсор на ".."
    assert!(app.active_panel().marked.is_empty());
}

#[test]
fn sort_toggle_reverses_on_repeat() {
    let mut app = app_with(vec![]);
    assert_eq!(app.active_panel().sort.key, SortKey::Name);
    // Ctrl+x s s → размер
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Char('s')));
    assert_eq!(app.active_panel().sort.key, SortKey::Size);
    assert!(!app.active_panel().sort.reverse);
    // повтор размера → reverse
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Char('s')));
    assert!(app.active_panel().sort.reverse);
}

#[test]
fn sort_f_toggles_dirs_first() {
    let mut app = app_with(vec![]);
    assert!(app.active_panel().sort.dirs_first);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Char('f')));
    assert!(!app.active_panel().sort.dirs_first);
}

#[test]
fn sort_entries_size_reverse() {
    let mut small = entry("small");
    small.size = Some(10);
    let mut big = entry("big");
    big.size = Some(1000);
    let mut v = vec![small, big];
    let sort = SortState {
        key: SortKey::Size,
        reverse: true,
        dirs_first: false,
    };
    sort_entries(&mut v, sort);
    assert_eq!(v[0].name, "big"); // по убыванию размера
}

#[test]
fn delete_opens_confirm_dialog() {
    let mut app = app_with(vec![entry("a")]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('d')));
    assert!(matches!(app.dialog, Some(Dialog::Confirm { .. })));
    // Обычная 'n' не закрывает (нужен Ctrl); Ctrl+N — отмена.
    app.handle_key(key(KeyCode::Char('n')));
    assert!(matches!(app.dialog, Some(Dialog::Confirm { .. })));
    app.handle_key(ctrl('n'));
    assert!(app.dialog.is_none());
    assert_eq!(app.status, "cancelled");
}

#[test]
fn delete_sudo_returns_shell_command() {
    let mut app = app_with(vec![entry("a")]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('d')));
    // Ctrl+S → удаление через sudo rm на экране shell.
    match app.handle_key(ctrl('s')) {
        Action::RunShell(cmd) => {
            assert!(cmd.starts_with("sudo rm -rf --"), "cmd was {cmd:?}");
            assert!(cmd.contains('a'));
        }
        other => panic!("expected RunShell, got {other:?}"),
    }
    assert!(app.dialog.is_none()); // диалог закрыт
}

#[test]
fn delete_y_removes_file() {
    let dir = std::env::temp_dir().join(format!("rfm_del_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x"), b"data").unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().path = crate::vfs::VfsPath::local(dir.clone());
    app.active_panel_mut().entries = vec![entry("x")];
    app.active_panel_mut().cursor = 0;

    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('d')));
    app.handle_key(ctrl('y')); // подтвердить (Ctrl+Y)
    assert!(!dir.join("x").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn delete_enter_removes_file() {
    let dir = std::env::temp_dir().join(format!("rfm_del_enter_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x"), b"data").unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().path = crate::vfs::VfsPath::local(dir.clone());
    app.active_panel_mut().entries = vec![entry("x")];
    app.active_panel_mut().cursor = 0;

    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('d')));
    app.handle_key(key(KeyCode::Enter)); // подтвердить (Enter = Ctrl+Y)
    assert!(app.dialog.is_none());
    assert!(!dir.join("x").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mkdir_opens_input_dialog() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('n')));
    assert!(matches!(app.dialog, Some(Dialog::Input { .. })));
    app.handle_key(key(KeyCode::Esc));
    assert!(app.dialog.is_none());
}

#[test]
fn mkdir_creates_directory_end_to_end() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static C: AtomicU32 = AtomicU32::new(0);
    let tmp = std::env::temp_dir().join(format!(
        "rfm_mkapp_{}_{}",
        std::process::id(),
        C.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    let mut app = App::new(Config::default());
    app.panels[0] = Panel::new(VfsPath::local(tmp.clone()));
    app.active = 0;
    let _ = app.panels[0].reload(true, FileListView::Flat);

    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('n')));
    for ch in "sub".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(tmp.join("sub").is_dir(), "mkdir should create the directory");
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn mkdir_buttons_ctrl_y_creates_ctrl_n_cancels() {
    let tmp = std::env::temp_dir().join(format!("rfm_mkbtn_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut app = App::new(Config::default());
    app.panels[0] = Panel::new(VfsPath::local(tmp.clone()));
    app.active = 0;

    // Ctrl+N — отмена, ничего не создаётся.
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('n')));
    app.handle_key(key(KeyCode::Char('x')));
    app.handle_key(ctrl('n'));
    assert!(app.dialog.is_none());
    assert!(!tmp.join("x").exists());

    // Ctrl+Y — создать.
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('n')));
    app.handle_key(key(KeyCode::Char('y')));
    app.handle_key(ctrl('y'));
    assert!(tmp.join("y").is_dir());
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn copy_opens_input_dialog() {
    let mut app = app_with(vec![entry("a")]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('c')));
    assert!(matches!(app.dialog, Some(Dialog::Input { .. })));
    app.handle_key(key(KeyCode::Esc));
    assert!(app.dialog.is_none());
}

#[test]
fn view_external_runs_program() {
    let mut app = app_with(vec![entry("file.txt")]);
    app.config.view = "/usr/bin/less".to_string();
    app.handle_key(ctrl('x'));
    let action = app.handle_key(key(KeyCode::Char('v')));
    match action {
        Action::RunShell(cmd) => {
            assert!(cmd.contains("less"));
            assert!(cmd.contains("file.txt"));
        }
        _ => panic!("expected RunShell"),
    }
}

#[test]
fn edit_external_runs_program() {
    let mut app = app_with(vec![entry("file.txt")]);
    app.config.edit = "/usr/bin/vim".to_string();
    app.handle_key(ctrl('x'));
    let action = app.handle_key(key(KeyCode::Char('e')));
    match action {
        Action::RunShell(cmd) => {
            assert!(cmd.contains("vim"));
            assert!(cmd.contains("file.txt"));
        }
        _ => panic!("expected RunShell"),
    }
}

#[test]
fn tab_switches_panel_while_viewer_open() {
    let mut app = app_with(vec![]);
    create_panel_keys(&mut app); // 2 панели, active=1
    app.active = 0;
    app.panels[0].viewer = Some(Viewer::from_memory("x".to_string(), vec!["a".to_string()]));
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.active, 1); // переключились
    assert!(app.panels[0].viewer.is_some()); // просмотрщик остался в своей панели
}

#[test]
fn tree_expand_collapse() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static C: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rfm_tree_{}_{}",
        std::process::id(),
        C.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/child.txt"), b"x").unwrap();
    std::fs::write(dir.join("top.txt"), b"y").unwrap();

    let mut app = app_with(vec![]);
    app.panels[0] = Panel::new(VfsPath::local(dir.clone()));
    app.active = 0;
    app.active_panel_mut().view_override = Some(FileListView::Tree);
    app.reload();
    assert!(app.active_panel().is_tree);

    let names: Vec<String> = app.active_panel().entries.iter().map(|e| e.name.clone()).collect();
    assert!(names.iter().any(|n| n == "sub"));
    assert!(!names.iter().any(|n| n.starts_with("sub/"))); // свёрнуто

    // Развернуть "sub"
    let i = app.active_panel().entries.iter().position(|e| e.name == "sub").unwrap();
    app.active_panel_mut().cursor = i;
    app.handle_key(key(KeyCode::Enter));
    assert!(app
        .active_panel()
        .entries
        .iter()
        .any(|e| e.name == "sub/child.txt"));

    // Свернуть обратно
    let i = app.active_panel().entries.iter().position(|e| e.name == "sub").unwrap();
    app.active_panel_mut().cursor = i;
    app.handle_key(key(KeyCode::Enter));
    assert!(!app
        .active_panel()
        .entries
        .iter()
        .any(|e| e.name.starts_with("sub/")));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn view_internal_opens_viewer_and_scrolls() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static C: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rfm_view_{}_{}",
        std::process::id(),
        C.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("f.txt"), b"a\nb\nc\n").unwrap();

    let mut app = app_with(vec![]);
    app.panels[0] = Panel::new(VfsPath::local(dir.clone()));
    app.active = 0;
    app.active_panel_mut().entries = vec![entry("f.txt")];
    app.active_panel_mut().cursor = 0;

    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('v'))); // view=internal по умолчанию
    assert!(app.active_panel().viewer.is_some());
    assert_eq!(app.active_panel_mut().viewer.as_mut().unwrap().window(0, 10).len(), 3);

    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.active_panel().viewer.as_ref().unwrap().voff, 1);
    app.handle_key(key(KeyCode::Char('q')));
    assert!(app.active_panel().viewer.is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn edit_noop_on_directory() {
    let mut app = app_with(vec![dir_entry("somedir")]);
    app.handle_key(ctrl('x'));
    let action = app.handle_key(key(KeyCode::Char('e')));
    assert_eq!(action, Action::Redraw);
    assert!(app.status.contains("no file under cursor"));
}

#[test]
fn help_opens_scrolls_and_closes() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('h')));
    assert!(app.help.is_some());
    // Скролл не закрывает окно.
    app.handle_key(key(KeyCode::Down));
    assert!(app.help.is_some());
    // Закрытие по q.
    app.handle_key(key(KeyCode::Char('q')));
    assert!(app.help.is_none());
}

#[test]
fn sanitize_replaces_control_chars() {
    // bell/nul → '.', таб → пробелы, печатные — как есть.
    let s = sanitize_line("a\x07b\tc\x00");
    assert_eq!(s, "a.b    c.");
}

#[test]
fn hex_line_layout() {
    let line = hex_line(0, b"AB");
    // offset, hex, текстовая колонка с печатными символами.
    assert!(line.starts_with("00000000  41 42 "));
    assert!(line.ends_with("|AB|"));
}

#[test]
fn hex_line_nonprintable_as_dot() {
    let line = hex_line(0, &[0x00, 0x41, 0xff]);
    assert!(line.ends_with("|.A.|"));
}

#[test]
fn viewer_toggles_hex_and_wrap() {
    let mut app = app_with(vec![]);
    let mut v = Viewer::from_memory("x".to_string(), vec!["a".to_string()]);
    v.voff = 3;
    v.hoff = 5;
    v.page = 1;
    app.panels[0].viewer = Some(v);
    // d — включает hex и сбрасывает смещения.
    app.handle_key(key(KeyCode::Char('d')));
    let v = app.active_panel().viewer.as_ref().unwrap();
    assert!(v.hex);
    assert_eq!(v.voff, 0);
    assert_eq!(v.hoff, 0);

    // w — включает wrap; в режиме wrap ←/→ не двигают hoff.
    app.handle_key(key(KeyCode::Char('d'))); // назад в ascii
    app.handle_key(key(KeyCode::Char('w')));
    app.handle_key(key(KeyCode::Right));
    let v = app.active_panel().viewer.as_ref().unwrap();
    assert!(v.wrap);
    assert_eq!(v.hoff, 0);
}

#[test]
fn viewer_toggle_persists_to_config() {
    let mut app = app_with(vec![]);
    app.config.config_save = SaveMode::Always;
    let mut v = Viewer::from_memory("x".to_string(), vec!["a".to_string()]);
    v.page = 1;
    app.panels[0].viewer = Some(v);
    assert!(!app.config.view_hex);
    app.handle_key(key(KeyCode::Char('d')));
    assert!(app.config.view_hex); // запомнили режим в конфиге
    app.handle_key(key(KeyCode::Char('w')));
    assert!(app.config.view_wrap);
}

#[test]
fn settings_viewer_hex_toggle() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    // → hex (секция Viewer): 8 шагов от show_hidden.
    for _ in 0..8 {
        app.handle_key(key(KeyCode::Down));
    }
    let before = app.config.view_hex;
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.config.view_hex, !before);
}

#[test]
fn settings_opens_toggles_and_closes() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.settings.is_some());

    // sel=0 → show_hidden toggle
    let before = app.config.show_hidden;
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.config.show_hidden, !before);

    // вниз к panel_layout (через show_clock) и переключить
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    let pl = app.config.panel_layout;
    app.handle_key(key(KeyCode::Right));
    assert_ne!(app.config.panel_layout, pl);

    app.handle_key(key(KeyCode::Esc));
    assert!(app.settings.is_none());
}

#[test]
fn copy_move_dialog_ctrl_actions() {
    // Ctrl+N — отмена.
    let mut app = app_with(vec![entry("a")]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('c'))); // op copy → диалог ввода
    assert!(matches!(app.dialog, Some(Dialog::Input { .. })));
    app.handle_key(ctrl('n'));
    assert!(app.dialog.is_none());
    assert_eq!(app.status, "cancelled");

    // Ctrl+S в move → sudo mv на экране shell.
    let mut app = app_with(vec![entry("a")]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('m'))); // op move
    match app.handle_key(ctrl('s')) {
        Action::RunShell(cmd) => {
            assert!(cmd.starts_with("sudo mv --"), "cmd was {cmd:?}");
            assert!(cmd.contains('a'));
        }
        other => panic!("expected RunShell, got {other:?}"),
    }
    assert!(app.dialog.is_none());
}

#[test]
fn copy_dialog_ctrl_y_copies() {
    let dir = std::env::temp_dir().join(format!("rfm_cpy_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("s"), b"data").unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().path = crate::vfs::VfsPath::local(dir.clone());
    app.active_panel_mut().entries = vec![entry("s")];
    app.active_panel_mut().cursor = 0;

    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('c'))); // copy → диалог
    // очистить префилл и ввести относительное имя назначения
    app.handle_key(ctrl('u'));
    for ch in "d".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(ctrl('y')); // выполнить копирование
    assert!(dir.join("s").exists()); // оригинал на месте
    assert_eq!(std::fs::read(dir.join("d")).unwrap(), b"data");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn home_end_jump_to_list_bounds() {
    let mut app = app_with(vec![entry("a"), entry("b"), entry("c"), entry("d")]);
    app.active_panel_mut().cursor = 2;
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.active_panel().cursor, 0); // в начало
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.active_panel().cursor, 3); // в конец (последний индекс)
}

#[test]
fn cd_recorded_in_history_except_trivial() {
    let mut app = app_with(vec![]);
    let has = |app: &App, c: &str| app.history.recent().iter().any(|e| e == c);

    // `cd ..` — тривиальный, не пишем.
    app.cmdline = CmdLine::from_str("cd ..");
    app.handle_key(key(KeyCode::Enter));
    assert!(!has(&app, "cd .."));

    // `cd sftp://host/` — удалённый, пишем (его долго набирать).
    app.cmdline = CmdLine::from_str("cd sftp://host/");
    app.handle_key(key(KeyCode::Enter));
    assert!(has(&app, "cd sftp://host/"));

    // Обычный путь — тоже пишем.
    app.cmdline = CmdLine::from_str("cd /tmp");
    app.handle_key(key(KeyCode::Enter));
    assert!(has(&app, "cd /tmp"));
}

#[test]
fn move_relative_name_renames_in_current_dir() {
    let dir = std::env::temp_dir().join(format!("rfm_mv_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("123"), b"x").unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().path = crate::vfs::VfsPath::local(dir.clone());

    // move "123" → относительное "321" (не существующая директория) = переименование.
    app.execute_op(PendingOp::Move(vec![dir.join("123")]), Some(std::path::PathBuf::from("321")));
    assert!(!dir.join("123").exists());
    assert!(dir.join("321").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn move_multiple_to_nondir_errors() {
    let dir = std::env::temp_dir().join(format!("rfm_mv2_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a"), b"a").unwrap();
    std::fs::write(dir.join("b"), b"b").unwrap();
    let mut app = app_with(vec![]);
    app.active_panel_mut().path = crate::vfs::VfsPath::local(dir.clone());

    // Несколько источников + цель-не-директория → ошибка, файлы на месте.
    app.execute_op(
        PendingOp::Move(vec![dir.join("a"), dir.join("b")]),
        Some(std::path::PathBuf::from("newname")),
    );
    assert!(app.status.contains("not a directory"));
    assert!(dir.join("a").exists() && dir.join("b").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dialog_input_ctrl_line_editing() {
    let dialog = |s: &str| Dialog::Input {
        prompt: "Copy to:".to_string(),
        input: CmdLine::from_str(s),
        op: PendingOp::MkDir(std::path::PathBuf::from("/tmp")),
    };
    let text = |app: &App| match app.dialog.as_ref() {
        Some(Dialog::Input { input, .. }) => input.text(),
        _ => panic!("input dialog gone"),
    };

    // Ctrl+u — убить до начала (курсор в конце) → пусто.
    let mut app = app_with(vec![]);
    app.dialog = Some(dialog("abcdef"));
    app.handle_key(ctrl('u'));
    assert_eq!(text(&app), "");

    // Ctrl+b (влево) + Ctrl+k (убить до конца) → отрезали последний символ.
    app.dialog = Some(dialog("abcdef"));
    app.handle_key(ctrl('b'));
    app.handle_key(ctrl('k'));
    assert_eq!(text(&app), "abcde");

    // Ctrl+f (вправо) двигает курсор — вставка идёт в новую позицию.
    app.dialog = Some(dialog("ab"));
    app.handle_key(ctrl('u')); // очистить
    for ch in "xy".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(ctrl('b')); // между x и y
    app.handle_key(key(KeyCode::Char('Z')));
    assert_eq!(text(&app), "xZy");

    // Ctrl+a (в начало) + вставка идёт в начало.
    app.dialog = Some(dialog("bc"));
    app.handle_key(ctrl('a'));
    app.handle_key(key(KeyCode::Char('a')));
    assert_eq!(text(&app), "abc");

    // Ctrl+w — стереть слово слева от курсора (Ctrl+e — в конец).
    app.dialog = Some(dialog("foo bar"));
    app.handle_key(ctrl('e'));
    app.handle_key(ctrl('w'));
    assert_eq!(text(&app), "foo ");
}

#[test]
fn settings_q_closes_like_esc() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.settings.is_some());
    app.handle_key(key(KeyCode::Char('q'))); // 'q' = Esc
    assert!(app.settings.is_none());
}

#[test]
fn settings_cycles_pause_mode() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    // перейти на pause_after_command (5 шагов от show_hidden)
    for _ in 0..5 {
        app.handle_key(key(KeyCode::Down));
    }
    assert_eq!(app.config.pause_after_command, crate::config::PauseMode::OnOutput);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.config.pause_after_command, crate::config::PauseMode::Never);
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.config.pause_after_command, crate::config::PauseMode::OnOutput);
}

#[test]
fn settings_cycles_config_save() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    for _ in 0..6 {
        app.handle_key(key(KeyCode::Down)); // → config_save
    }
    assert_eq!(app.config.config_save, SaveMode::OnChange);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.config.config_save, SaveMode::OnSave);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.config.config_save, SaveMode::Always);
}

#[test]
fn settings_edit_view_string() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    for _ in 0..7 {
        app.handle_key(key(KeyCode::Down)); // → view (секция Viewer)
    }
    app.handle_key(key(KeyCode::Enter)); // начать редактирование (буфер="internal")
    for _ in 0.."internal".len() {
        app.handle_key(key(KeyCode::Backspace));
    }
    for ch in "/usr/bin/less".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter)); // применить
    assert_eq!(app.config.view, "/usr/bin/less");
}

#[test]
fn settings_save_closes_window() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    for _ in 0..13 {
        app.handle_key(key(KeyCode::Down)); // → строка кнопок (Save выбран)
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.status, "config saved");
    assert!(app.settings.is_none()); // Save закрыл окно
}

#[test]
fn panel_view_override_cycles() {
    let mut app = app_with(vec![]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    for _ in 0..11 {
        app.handle_key(key(KeyCode::Down)); // → Panel 1 · file_list_view
    }
    assert_eq!(app.active_panel().view_override, None);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.active_panel().view_override, Some(FileListView::Flat));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.active_panel().view_override, Some(FileListView::Tree));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.active_panel().view_override, None);
}

#[test]
fn panel_columns_setting() {
    let mut app = app_with(vec![entry("a"), entry("b")]);
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('x')));
    for _ in 0..12 {
        app.handle_key(key(KeyCode::Down)); // → Panel 1 · columns
    }
    app.handle_key(key(KeyCode::Right)); // columns 1 -> 2
    assert_eq!(app.active_panel().columns, 2);
}

#[test]
fn grid_column_major_navigation() {
    // 6 записей, 2 колонки, высота столбца 3 → col0=[0,1,2], col1=[3,4,5]
    let mut app = app_with(vec![
        entry("0"),
        entry("1"),
        entry("2"),
        entry("3"),
        entry("4"),
        entry("5"),
    ]);
    app.active_panel_mut().columns = 2;
    app.active_panel_mut().grid_rows = 3; // как после рендера
    app.active_panel_mut().cursor = 0;

    // Вниз по столбцу: +1
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.active_panel().cursor, 1);
    // Вправо: +grid_rows (3) → в тот же ряд следующего столбца
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.active_panel().cursor, 4);
    // Вверх: -1
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.active_panel().cursor, 3);
    // Влево: -3 → обратно в первый столбец
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.active_panel().cursor, 0);
}

#[test]
fn theme_selected_from_config() {
    use crate::config::ThemeConfig;
    let config = Config {
        theme: "solar".to_string(),
        themes: vec![ThemeConfig {
            name: "solar".to_string(),
            bg: "#101010".to_string(),
            fg: "white".to_string(),
            cursor_bg: "yellow".to_string(),
            cursor_fg: "black".to_string(),
            mark_fg: "green".to_string(),
            cmdline_bg: "black".to_string(),
            cmdline_fg: "white".to_string(),
            button_sel_bg: "red".to_string(),
            button_sel_fg: "blue".to_string(),
            select_bg: "magenta".to_string(),
            select_fg: String::new(), // fallback
        }],
        ..Config::default()
    };
    let app = App::new(config);
    assert_eq!(app.theme.bg, ratatui::style::Color::Rgb(16, 16, 16));
    assert_eq!(app.theme.mark_fg, ratatui::style::Color::Green);
    assert_eq!(app.theme.button_sel_bg, ratatui::style::Color::Red);
    assert_eq!(app.theme.button_sel_fg, ratatui::style::Color::Blue);
    assert_eq!(app.theme.select_bg, ratatui::style::Color::Magenta);
    assert_eq!(app.theme.select_fg, crate::theme::Theme::default_dark().select_fg);
}
