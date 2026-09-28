//! Экран настроек (`Ctrl+x x`): навигация по секциям и применение изменений.

use super::*;

impl App {
    pub(super) fn theme_names(&self) -> Vec<String> {
        if self.config.themes.is_empty() {
            vec!["default".to_string()]
        } else {
            self.config.themes.iter().map(|t| t.name.clone()).collect()
        }
    }

    pub(super) fn handle_settings_key(&mut self, key: KeyEvent) -> Action {
        // Режим редактирования строкового значения (view/edit).
        if self.settings.as_ref().map(|s| s.editing.is_some()).unwrap_or(false) {
            return self.handle_settings_edit_key(key);
        }
        let sel = self.settings.as_ref().map(|s| s.sel).unwrap_or(0);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                // `q` работает как Esc (закрыть окно настроек).
                // При on-change сохраняем накопленные изменения.
                let dirty = self.settings.as_ref().map(|s| s.dirty).unwrap_or(false);
                self.settings = None;
                if dirty && self.config.config_save == SaveMode::OnChange {
                    self.save_config();
                }
            }
            KeyCode::Up => self.settings_move(-1),
            KeyCode::Down => self.settings_move(1),
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => {
                if let Some(id) = setting_at(sel) {
                    self.settings_change(id, 1);
                }
            }
            KeyCode::Left => {
                if let Some(id) = setting_at(sel) {
                    self.settings_change(id, -1);
                }
            }
            _ => {}
        }
        Action::Redraw
    }

    pub(super) fn settings_move(&mut self, dir: isize) {
        if let Some(s) = self.settings.as_mut() {
            let n = SETTINGS_ROWS.len() as isize;
            let mut i = s.sel as isize;
            loop {
                i += dir;
                if i < 0 || i >= n {
                    return;
                }
                if matches!(SETTINGS_ROWS[i as usize], SettingRow::Field(_)) {
                    s.sel = i as usize;
                    return;
                }
            }
        }
    }

    pub(super) fn handle_settings_edit_key(&mut self, key: KeyEvent) -> Action {
        let sel = self.settings.as_ref().map(|s| s.sel).unwrap_or(0);
        match key.code {
            KeyCode::Enter => {
                if let Some(s) = self.settings.as_mut() {
                    if let Some(buf) = s.editing.take() {
                        let val = buf.text();
                        match setting_at(sel) {
                            Some(SettingId::View) => self.config.view = val,
                            Some(SettingId::Edit) => self.config.edit = val,
                            _ => {}
                        }
                        self.mark_settings_dirty();
                    }
                }
            }
            KeyCode::Esc => {
                if let Some(s) = self.settings.as_mut() {
                    s.editing = None;
                }
            }
            KeyCode::Backspace => {
                if let Some(b) = self.settings.as_mut().and_then(|s| s.editing.as_mut()) {
                    b.backspace();
                }
            }
            KeyCode::Left => {
                if let Some(b) = self.settings.as_mut().and_then(|s| s.editing.as_mut()) {
                    b.left();
                }
            }
            KeyCode::Right => {
                if let Some(b) = self.settings.as_mut().and_then(|s| s.editing.as_mut()) {
                    b.right();
                }
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(b) = self.settings.as_mut().and_then(|s| s.editing.as_mut()) {
                    b.insert(c);
                }
            }
            _ => {}
        }
        Action::Redraw
    }

    pub(super) fn settings_change(&mut self, id: SettingId, dir: i32) {
        match id {
            SettingId::ShowHidden => {
                self.config.show_hidden = !self.config.show_hidden;
                self.reload_all();
                self.mark_settings_dirty();
            }
            SettingId::ShowClock => {
                self.config.show_clock = !self.config.show_clock;
                self.mark_settings_dirty();
            }
            SettingId::PanelLayout => {
                self.config.panel_layout = match self.config.panel_layout {
                    PanelLayout::Vertical => PanelLayout::Horizontal,
                    PanelLayout::Horizontal => PanelLayout::Vertical,
                };
                self.mark_settings_dirty();
            }
            SettingId::FileListView => {
                self.config.file_list_view = match self.config.file_list_view {
                    FileListView::Flat => FileListView::Tree,
                    FileListView::Tree => FileListView::Flat,
                };
                self.mark_settings_dirty();
            }
            SettingId::Theme => {
                self.cycle_theme(dir);
                self.mark_settings_dirty();
            }
            SettingId::PauseAfterCommand => {
                let order = [PauseMode::Always, PauseMode::OnOutput, PauseMode::Never];
                let cur = order
                    .iter()
                    .position(|m| *m == self.config.pause_after_command)
                    .unwrap_or(1);
                let next = (cur as i32 + dir).rem_euclid(order.len() as i32) as usize;
                self.config.pause_after_command = order[next];
                self.mark_settings_dirty();
            }
            SettingId::ConfigSave => {
                let order = [SaveMode::Always, SaveMode::OnChange, SaveMode::OnSave];
                let cur = order
                    .iter()
                    .position(|m| *m == self.config.config_save)
                    .unwrap_or(1);
                let next = (cur as i32 + dir).rem_euclid(order.len() as i32) as usize;
                self.config.config_save = order[next];
                self.mark_settings_dirty();
            }
            SettingId::View => {
                let cur = self.config.view.clone();
                if let Some(s) = self.settings.as_mut() {
                    s.editing = Some(CmdLine::from_str(&cur));
                }
            }
            SettingId::Edit => {
                let cur = self.config.edit.clone();
                if let Some(s) = self.settings.as_mut() {
                    s.editing = Some(CmdLine::from_str(&cur));
                }
            }
            SettingId::ViewHex => {
                self.config.view_hex = !self.config.view_hex;
                self.mark_settings_dirty();
            }
            SettingId::ViewWrap => {
                self.config.view_wrap = !self.config.view_wrap;
                self.mark_settings_dirty();
            }
            // Настройки текущей панели (не в конфиге — сессионные, dirty не ставим).
            SettingId::PanelView => {
                let next = match self.active_panel().view_override {
                    None => Some(FileListView::Flat),
                    Some(FileListView::Flat) => Some(FileListView::Tree),
                    Some(FileListView::Tree) => None,
                };
                self.active_panel_mut().view_override = next;
                self.reload();
                self.panels_dirty = true;
            }
            SettingId::PanelColumns => {
                let cur = self.active_panel().columns as i32;
                let next = (cur + dir).clamp(1, MAX_COLUMNS as i32) as usize;
                let p = self.active_panel_mut();
                p.columns = next;
                p.grid_left = 0;
                self.panels_dirty = true;
            }
            SettingId::Save => {
                self.save_config();
                self.settings = None; // Save закрывает окно
            }
        }
    }

    pub(super) fn mark_settings_dirty(&mut self) {
        if let Some(s) = self.settings.as_mut() {
            s.dirty = true;
        }
        if self.config.config_save == SaveMode::Always {
            self.save_config();
        }
    }

    pub(super) fn persist_view_pref(&mut self) {
        if matches!(self.config.config_save, SaveMode::Always | SaveMode::OnChange) {
            self.save_config();
        }
    }

    pub(super) fn save_config(&mut self) {
        if cfg!(test) {
            self.status = "config saved".to_string();
            if let Some(s) = self.settings.as_mut() {
                s.dirty = false;
            }
            return;
        }
        match config::save(&self.config) {
            Ok(()) => {
                self.status = "config saved".to_string();
                if let Some(s) = self.settings.as_mut() {
                    s.dirty = false;
                }
            }
            Err(e) => self.status = format!("save config: {e}"),
        }
    }

    pub(super) fn cycle_theme(&mut self, dir: i32) {
        let names = self.theme_names();
        let cur = names
            .iter()
            .position(|n| *n == self.config.theme)
            .unwrap_or(0);
        let next = (cur as i32 + dir).rem_euclid(names.len() as i32) as usize;
        self.config.theme = names[next].clone();
        self.theme = resolve_theme(&self.config);
    }
}

/// Состояние экрана настроек (`Ctrl+x x`).
pub struct SettingsState {
    pub sel: usize,
    /// Были ли изменения (для режима сохранения `on-change`).
    pub dirty: bool,
    /// Редактирование строкового значения (view/edit): буфер ввода.
    pub editing: Option<CmdLine>,
}

/// Настраиваемый параметр в окне настроек.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingId {
    // Global
    ShowHidden,
    ShowClock,
    PanelLayout,
    FileListView,
    Theme,
    PauseAfterCommand,
    ConfigSave,
    // Current panel
    PanelView,
    PanelColumns,
    // Viewer
    View,
    ViewHex,
    ViewWrap,
    // Editor
    Edit,
    // Action
    Save,
}

/// Строка экрана настроек: заголовок секции (не выбирается) либо поле.
pub enum SettingRow {
    Header(&'static str),
    Field(SettingId),
}

/// Полный порядок строк окна настроек (с заголовками секций).
pub const SETTINGS_ROWS: &[SettingRow] = &[
    SettingRow::Header("Global"),
    SettingRow::Field(SettingId::ShowHidden),
    SettingRow::Field(SettingId::ShowClock),
    SettingRow::Field(SettingId::PanelLayout),
    SettingRow::Field(SettingId::FileListView),
    SettingRow::Field(SettingId::Theme),
    SettingRow::Field(SettingId::PauseAfterCommand),
    SettingRow::Field(SettingId::ConfigSave),
    SettingRow::Header("Current panel"),
    SettingRow::Field(SettingId::PanelView),
    SettingRow::Field(SettingId::PanelColumns),
    SettingRow::Header("Viewer"),
    SettingRow::Field(SettingId::View),
    SettingRow::Field(SettingId::ViewHex),
    SettingRow::Field(SettingId::ViewWrap),
    SettingRow::Header("Editor"),
    SettingRow::Field(SettingId::Edit),
    SettingRow::Field(SettingId::Save),
];

/// Индекс первого выбираемого поля (строка 0 — заголовок «Global»).
pub const SETTINGS_FIRST: usize = 1;

/// Поле в строке `sel`, если это не заголовок.
pub(super) fn setting_at(sel: usize) -> Option<SettingId> {
    match SETTINGS_ROWS.get(sel)? {
        SettingRow::Field(id) => Some(*id),
        SettingRow::Header(_) => None,
    }
}

/// Максимум колонок в панели.
pub(super) const MAX_COLUMNS: usize = 6;
