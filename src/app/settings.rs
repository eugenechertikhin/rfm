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
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let rows = self.settings_rows();
        let on_buttons = rows.get(sel) == Some(&SettingRow::Buttons);
        match key.code {
            // Save (c-s) — сохранить и закрыть; Cancel (c-n) / Esc / q — откатить сеанс и закрыть.
            KeyCode::Char('s') if ctrl => self.settings_save(),
            KeyCode::Char('n') if ctrl => self.settings_cancel(),
            KeyCode::Esc | KeyCode::Char('q') if !ctrl => self.settings_cancel(),
            KeyCode::Up => self.settings_move(-1),
            KeyCode::Down => self.settings_move(1),
            KeyCode::Tab => self.settings_section(1),
            KeyCode::BackTab => self.settings_section(-1),
            // На строке кнопок: ←/→ — выбор кнопки, Enter/Space — нажать.
            KeyCode::Left | KeyCode::Right if on_buttons => {
                if let Some(s) = self.settings.as_mut() {
                    s.button = 1 - s.button.min(1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') if on_buttons => {
                match self.settings.as_ref().map(|s| s.button).unwrap_or(0) {
                    0 => self.settings_save(),
                    _ => self.settings_cancel(),
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => {
                if let Some(id) = setting_at(&rows, sel) {
                    self.settings_change(id, 1);
                }
            }
            KeyCode::Left => {
                if let Some(id) = setting_at(&rows, sel) {
                    self.settings_change(id, -1);
                }
            }
            _ => {}
        }
        Action::Redraw
    }

    /// `[ Save ]`: записать конфиг и закрыть окно.
    pub(super) fn settings_save(&mut self) {
        self.save_config();
        self.settings = None;
    }

    /// `[ Cancel ]` / `Esc`: откатить всё, что изменено за сеанс окна, и закрыть его.
    /// Если за сеанс конфиг уже писался на диск (`config_save = always`), записываем
    /// восстановленный.
    pub(super) fn settings_cancel(&mut self) {
        let Some(s) = self.settings.take() else { return };
        if !s.dirty && !s.saved && self.panel_settings() == s.orig_panels {
            return; // ничего не менялось
        }
        self.config = s.orig_config;
        self.theme = resolve_theme(&self.config);
        if self.panel_settings() != s.orig_panels {
            for (p, (view, cols)) in self.panels.iter_mut().zip(s.orig_panels) {
                if p.columns != cols {
                    p.grid_left = 0;
                }
                p.view_override = view;
                p.columns = cols;
            }
            self.panels_dirty = true;
        }
        self.reload_all();
        if s.saved {
            self.save_config();
        }
    }

    /// Настройки панелей, меняемые в окне: (override вида, число колонок).
    pub(super) fn panel_settings(&self) -> Vec<(Option<FileListView>, usize)> {
        self.panels.iter().map(|p| (p.view_override, p.columns)).collect()
    }

    /// Строки окна настроек для текущего набора панелей.
    pub fn settings_rows(&self) -> Vec<SettingRow> {
        settings_rows(self.panels.len())
    }

    pub(super) fn settings_move(&mut self, dir: isize) {
        let rows = self.settings_rows();
        if let Some(s) = self.settings.as_mut() {
            let n = rows.len() as isize;
            let mut i = s.sel as isize;
            loop {
                i += dir;
                if i < 0 || i >= n {
                    return;
                }
                if matches!(rows[i as usize], SettingRow::Field(_) | SettingRow::Buttons) {
                    s.sel = i as usize;
                    return;
                }
            }
        }
    }

    /// `Tab` / `Shift+Tab` — на первое поле следующего / предыдущего раздела
    /// (Global → Viewer → Editor → Panel 1 → … → кнопки), по кругу.
    pub(super) fn settings_section(&mut self, dir: isize) {
        let starts = section_starts(&self.settings_rows());
        let Some(s) = self.settings.as_mut() else { return };
        let cur = starts.iter().rposition(|&i| i <= s.sel).unwrap_or(0) as isize;
        let next = (cur + dir).rem_euclid(starts.len() as isize) as usize;
        s.sel = starts[next];
    }

    pub(super) fn handle_settings_edit_key(&mut self, key: KeyEvent) -> Action {
        let sel = self.settings.as_ref().map(|s| s.sel).unwrap_or(0);
        match key.code {
            KeyCode::Enter => {
                if let Some(s) = self.settings.as_mut() {
                    if let Some(buf) = s.editing.take() {
                        let val = buf.text();
                        match setting_at(&settings_rows(self.panels.len()), sel) {
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
            // Настройки панели `i` (не в конфиге — в файле состояния панелей, dirty не ставим).
            SettingId::PanelView(i) => {
                let Some(p) = self.panels.get_mut(i) else { return };
                p.view_override = match p.view_override {
                    None => Some(FileListView::Flat),
                    Some(FileListView::Flat) => Some(FileListView::Tree),
                    Some(FileListView::Tree) => None,
                };
                let show_hidden = self.config.show_hidden;
                let view = self.effective_view(i);
                if let Err(e) = self.panels[i].reload(show_hidden, view) {
                    self.status = format!("cannot read directory: {e}");
                }
                self.panels_dirty = true;
            }
            SettingId::PanelColumns(i) => {
                let Some(p) = self.panels.get_mut(i) else { return };
                p.columns = (p.columns as i32 + dir).clamp(1, MAX_COLUMNS as i32) as usize;
                p.grid_left = 0;
                self.panels_dirty = true;
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
                s.saved = true;
            }
            return;
        }
        match config::save(&self.config) {
            Ok(()) => {
                self.status = "config saved".to_string();
                if let Some(s) = self.settings.as_mut() {
                    s.dirty = false;
                    s.saved = true;
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
    /// Были ли несохранённые изменения конфига за сеанс окна.
    pub dirty: bool,
    /// Писался ли конфиг на диск за сеанс окна (для отката по Cancel).
    pub saved: bool,
    /// Редактирование строкового значения (view/edit): буфер ввода.
    pub editing: Option<CmdLine>,
    /// Выбранная кнопка на строке кнопок: 0 — Save, 1 — Cancel.
    pub button: usize,
    /// Снимок конфига и настроек панелей на момент открытия — для отката по Cancel/Esc.
    pub orig_config: Config,
    pub orig_panels: Vec<(Option<FileListView>, usize)>,
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
    // Panel N (индекс панели)
    PanelView(usize),
    PanelColumns(usize),
    // Viewer
    View,
    ViewHex,
    ViewWrap,
    // Editor
    Edit,
}

/// Строка экрана настроек: заголовок секции (не выбирается), заголовок/путь панели
/// (не выбираются) либо поле.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRow {
    Header(&'static str),
    /// `── Panel N ──` (для активной — с пометкой `(active)`).
    PanelHeader(usize),
    /// Путь панели — информационная строка.
    PanelDir(usize),
    Field(SettingId),
    /// Строка кнопок `[ Save (c-s) ]  [ Cancel (c-n) ]` (последняя).
    Buttons,
}

/// Левая колонка окна настроек: Global, Viewer, Editor.
pub const SETTINGS_LEFT: &[SettingRow] = &[
    SettingRow::Header("Global"),
    SettingRow::Field(SettingId::ShowHidden),
    SettingRow::Field(SettingId::ShowClock),
    SettingRow::Field(SettingId::PanelLayout),
    SettingRow::Field(SettingId::FileListView),
    SettingRow::Field(SettingId::Theme),
    SettingRow::Field(SettingId::PauseAfterCommand),
    SettingRow::Field(SettingId::ConfigSave),
    SettingRow::Header("Viewer"),
    SettingRow::Field(SettingId::View),
    SettingRow::Field(SettingId::ViewHex),
    SettingRow::Field(SettingId::ViewWrap),
    SettingRow::Header("Editor"),
    SettingRow::Field(SettingId::Edit),
];

/// Полный порядок строк окна настроек: левая колонка, затем секции панелей
/// (правая колонка), последней — строка кнопок. Порядок = порядок `↑`/`↓`.
pub fn settings_rows(n_panels: usize) -> Vec<SettingRow> {
    let mut rows = SETTINGS_LEFT.to_vec();
    for i in 0..n_panels {
        rows.push(SettingRow::PanelHeader(i));
        rows.push(SettingRow::PanelDir(i));
        rows.push(SettingRow::Field(SettingId::PanelView(i)));
        rows.push(SettingRow::Field(SettingId::PanelColumns(i)));
    }
    rows.push(SettingRow::Buttons);
    rows
}

/// Точки входа разделов для `Tab`: первое поле после каждого заголовка
/// (`── Global ──`, …, `── Panel N ──`) и строка кнопок.
pub(super) fn section_starts(rows: &[SettingRow]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut want = false;
    for (i, r) in rows.iter().enumerate() {
        match r {
            SettingRow::Header(_) | SettingRow::PanelHeader(_) => want = true,
            SettingRow::Field(_) if want => {
                out.push(i);
                want = false;
            }
            SettingRow::Buttons => out.push(i),
            _ => {}
        }
    }
    out
}

/// Индекс первого выбираемого поля (строка 0 — заголовок «Global»).
pub const SETTINGS_FIRST: usize = 1;

/// Поле в строке `sel`, если это не заголовок.
pub(super) fn setting_at(rows: &[SettingRow], sel: usize) -> Option<SettingId> {
    match rows.get(sel)? {
        SettingRow::Field(id) => Some(*id),
        _ => None,
    }
}

/// Максимум колонок в панели.
pub(super) const MAX_COLUMNS: usize = 6;

#[cfg(test)]
#[path = "settings_test.rs"]
mod tests;
