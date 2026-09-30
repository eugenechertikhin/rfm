//! Тесты окна настроек: секции панелей, Tab по разделам, кнопки Save/Cancel.

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Приложение с `n` панелями в cwd и открытыми настройками.
fn app_panels(n: usize) -> App {
    let mut app = App::new(Config::default());
    while app.panels.len() < n {
        let p = Panel::new(app.panels[0].path.clone());
        app.panels.push(p);
    }
    app.open_settings();
    app
}

fn sel(app: &App) -> usize {
    app.settings.as_ref().unwrap().sel
}

fn row_of(app: &App, id: SettingId) -> usize {
    app.settings_rows()
        .iter()
        .position(|r| *r == SettingRow::Field(id))
        .unwrap()
}

#[test]
fn rows_have_section_per_panel_and_buttons_last() {
    let rows = settings_rows(3);
    let left = SETTINGS_LEFT.len();
    assert_eq!(rows.len(), left + 3 * 4 + 1);
    assert_eq!(rows[left], SettingRow::PanelHeader(0));
    assert_eq!(rows[left + 1], SettingRow::PanelDir(0));
    assert_eq!(rows[left + 2], SettingRow::Field(SettingId::PanelView(0)));
    assert_eq!(rows[left + 3], SettingRow::Field(SettingId::PanelColumns(0)));
    assert_eq!(rows[left + 8], SettingRow::PanelHeader(2));
    assert_eq!(*rows.last().unwrap(), SettingRow::Buttons);
    // В левой колонке панелей нет.
    assert!(!SETTINGS_LEFT
        .iter()
        .any(|r| matches!(r, SettingRow::PanelHeader(_) | SettingRow::PanelDir(_))));
}

#[test]
fn down_skips_panel_header_and_dir() {
    let mut app = app_panels(2);
    app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::PanelColumns(0));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(sel(&app), row_of(&app, SettingId::PanelView(1)));
}

#[test]
fn changes_apply_to_own_panel_not_active() {
    let mut app = app_panels(2);
    app.active = 0;
    app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::PanelColumns(1));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.panels[1].columns, 2);
    assert_eq!(app.panels[0].columns, 1);

    app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::PanelView(1));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.panels[1].view_override, Some(FileListView::Flat));
    assert_eq!(app.panels[0].view_override, None);
}

#[test]
fn columns_clamped_to_range() {
    let mut app = app_panels(1);
    app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::PanelColumns(0));
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.panels[0].columns, 1);
    for _ in 0..10 {
        app.handle_key(key(KeyCode::Right));
    }
    assert_eq!(app.panels[0].columns, MAX_COLUMNS);
}

#[test]
fn tab_cycles_sections() {
    let mut app = app_panels(2);
    let buttons = app.settings_rows().len() - 1;
    let order = [
        row_of(&app, SettingId::View),        // Viewer
        row_of(&app, SettingId::Edit),        // Editor
        row_of(&app, SettingId::PanelView(0)), // Panel 1
        row_of(&app, SettingId::PanelView(1)), // Panel 2
        buttons,                              // кнопки
        row_of(&app, SettingId::ShowHidden),  // по кругу — Global
    ];
    for want in order {
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(sel(&app), want);
    }
    // Из середины раздела — в начало следующего; Shift+Tab — назад.
    app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::Theme);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(sel(&app), row_of(&app, SettingId::View));
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(sel(&app), row_of(&app, SettingId::ShowHidden));
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(sel(&app), buttons);
}

#[test]
fn ctrl_s_saves_and_closes() {
    let mut app = app_panels(1);
    app.handle_key(key(KeyCode::Enter)); // show_hidden
    let changed = app.config.show_hidden;
    app.handle_key(ctrl('s'));
    assert!(app.settings.is_none());
    assert_eq!(app.status, "config saved");
    assert_eq!(app.config.show_hidden, changed);
}

#[test]
fn cancel_and_esc_revert_session_changes() {
    for close in [ctrl('n'), key(KeyCode::Esc), key(KeyCode::Char('q'))] {
        let mut app = app_panels(2);
        let hidden = app.config.show_hidden;
        let theme = app.config.theme.clone();
        app.handle_key(key(KeyCode::Enter)); // show_hidden
        app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::Theme);
        app.handle_key(key(KeyCode::Right));
        app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::PanelColumns(1));
        app.handle_key(key(KeyCode::Right));
        app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::PanelView(0));
        app.handle_key(key(KeyCode::Right));
        assert_ne!(app.config.show_hidden, hidden);

        app.handle_key(close);
        assert!(app.settings.is_none());
        assert_eq!(app.config.show_hidden, hidden);
        assert_eq!(app.config.theme, theme);
        assert_eq!(app.panels[1].columns, 1);
        assert_eq!(app.panels[0].view_override, None);
    }
}

#[test]
fn cancel_marks_panels_dirty_when_panel_settings_reverted() {
    let mut app = app_panels(1);
    app.settings.as_mut().unwrap().sel = row_of(&app, SettingId::PanelColumns(0));
    app.settings_change(SettingId::PanelColumns(0), 1);
    app.panels_dirty = false;
    app.settings_cancel();
    assert_eq!(app.panels[0].columns, 1);
    assert!(app.panels_dirty); // откат панелей будет записан в файл состояния
}

#[test]
fn cancel_rewrites_config_if_saved_during_session() {
    let mut app = app_panels(1);
    app.config.config_save = SaveMode::Always;
    app.open_settings();
    app.handle_key(key(KeyCode::Enter)); // show_hidden → сразу записано
    app.status.clear();
    app.handle_key(ctrl('n'));
    assert_eq!(app.status, "config saved"); // записан восстановленный
    // Без изменений Cancel ничего не пишет.
    app.open_settings();
    app.status.clear();
    app.handle_key(key(KeyCode::Esc));
    assert!(app.status.is_empty());
}

#[test]
fn buttons_row_arrows_pick_button_enter_presses() {
    let mut app = app_panels(1);
    let buttons = app.settings_rows().len() - 1;
    app.handle_key(key(KeyCode::Enter)); // show_hidden
    let hidden = app.config.show_hidden;
    app.settings.as_mut().unwrap().sel = buttons;
    app.handle_key(key(KeyCode::Right)); // → Cancel
    assert_eq!(app.settings.as_ref().unwrap().button, 1);
    assert_eq!(app.config.show_hidden, hidden); // стрелки не меняют настройки
    app.handle_key(key(KeyCode::Enter));
    assert!(app.settings.is_none());
    assert_ne!(app.config.show_hidden, hidden); // Cancel откатил

    let mut app = app_panels(1);
    app.handle_key(key(KeyCode::Enter));
    let hidden = app.config.show_hidden;
    app.settings.as_mut().unwrap().sel = buttons;
    app.handle_key(key(KeyCode::Enter)); // Save по умолчанию
    assert!(app.settings.is_none());
    assert_eq!(app.config.show_hidden, hidden);
    assert_eq!(app.status, "config saved");
}
