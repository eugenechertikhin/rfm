//! Встроенный простейший редактор (edit = internal): весь файл в памяти,
//! стрелки двигают курсор по символам (`→` в конце строки — на начало следующей),
//! печатные символы вставляются в позицию курсора. Правки живут в буфере;
//! на диск — только по `Ctrl+x s` (сохранить) / `Ctrl+x x` (сохранить и выйти).
//! `Esc`/`Ctrl+x q` — выход БЕЗ сохранения (несохранённое отбрасывается); при
//! несохранённых правках — сначала диалог подтверждения.

use super::*;
use std::fs;
use unicode_width::UnicodeWidthChar;

/// Подсказка в строке статуса при открытом редакторе.
pub(super) const EDITOR_HINT: &str =
    "edit: Esc close · arrows move · type to insert · c-x s save · c-x x save+close";
/// Подсказка аккорда `Ctrl+x` в редакторе.
const HINT_EDITOR: &str = "c-x: s save · x save+close · q close · h help";

/// Встроенный редактор текста файла.
pub struct Editor {
    pub name: String,
    /// Полный путь к файлу.
    pub path: PathBuf,
    /// Строки файла (без `\n`).
    pub lines: Vec<String>,
    /// Курсор: индекс строки и символа (char, не байт) в ней.
    pub cur_line: usize,
    pub cur_col: usize,
    /// Смещения прокрутки (строк / символов).
    pub voff: usize,
    pub hoff: usize,
    /// Размеры видимой области — обновляются при рендере (для PgUp/PgDn и прокрутки).
    pub page: usize,
    pub cols: usize,
    /// Есть ли несохранённые изменения (маркер `*` в заголовке).
    pub dirty: bool,
    /// Был ли у файла завершающий `\n` (сохраняем как было).
    trailing_newline: bool,
}

impl Editor {
    /// Загружает файл целиком в память (не-UTF8 байты — lossy).
    pub fn load(path: &std::path::Path) -> std::io::Result<Editor> {
        let bytes = fs::read(path)?;
        let text = String::from_utf8_lossy(&bytes);
        let trailing_newline = text.is_empty() || text.ends_with('\n');
        let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        // После завершающего `\n` split даёт пустой «хвост» — это не строка файла.
        if text.ends_with('\n') {
            lines.pop();
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Editor {
            name,
            path: path.to_path_buf(),
            lines,
            cur_line: 0,
            cur_col: 0,
            voff: 0,
            hoff: 0,
            page: 0,
            cols: 0,
            dirty: false,
            trailing_newline,
        })
    }

    /// Пишет содержимое на диск (целиком).
    pub fn save(&self) -> std::io::Result<()> {
        let mut text = self.lines.join("\n");
        if self.trailing_newline {
            text.push('\n');
        }
        fs::write(&self.path, text)
    }

    /// Длина строки `i` в символах.
    fn line_len(&self, i: usize) -> usize {
        self.lines.get(i).map(|l| l.chars().count()).unwrap_or(0)
    }

    /// Байтовое смещение символа `col` в строке `line` (для вставки/удаления).
    fn byte_at(&self, line: usize, col: usize) -> usize {
        let s = &self.lines[line];
        s.char_indices().nth(col).map(|(b, _)| b).unwrap_or(s.len())
    }

    pub fn move_left(&mut self) {
        if self.cur_col > 0 {
            self.cur_col -= 1;
        } else if self.cur_line > 0 {
            // В начале строки — на конец предыдущей.
            self.cur_line -= 1;
            self.cur_col = self.line_len(self.cur_line);
        }
    }

    pub fn move_right(&mut self) {
        if self.cur_col < self.line_len(self.cur_line) {
            self.cur_col += 1;
        } else if self.cur_line + 1 < self.lines.len() {
            // В конце строки — на начало следующей.
            self.cur_line += 1;
            self.cur_col = 0;
        }
    }

    pub fn move_up(&mut self, n: usize) {
        self.cur_line = self.cur_line.saturating_sub(n);
        self.cur_col = self.cur_col.min(self.line_len(self.cur_line));
    }

    pub fn move_down(&mut self, n: usize) {
        self.cur_line = (self.cur_line + n).min(self.lines.len().saturating_sub(1));
        self.cur_col = self.cur_col.min(self.line_len(self.cur_line));
    }

    /// Вставляет символ в позицию курсора; курсор — за вставленным.
    pub fn insert_char(&mut self, c: char) {
        let b = self.byte_at(self.cur_line, self.cur_col);
        self.lines[self.cur_line].insert(b, c);
        self.cur_col += 1;
    }

    /// Разрывает строку по курсору (Enter).
    pub fn insert_newline(&mut self) {
        let b = self.byte_at(self.cur_line, self.cur_col);
        let rest = self.lines[self.cur_line].split_off(b);
        self.lines.insert(self.cur_line + 1, rest);
        self.cur_line += 1;
        self.cur_col = 0;
    }

    /// Удаляет символ слева от курсора; в начале строки — склеивает с предыдущей.
    /// Возвращает `false`, если удалять нечего (начало файла).
    pub fn backspace(&mut self) -> bool {
        if self.cur_col > 0 {
            let b = self.byte_at(self.cur_line, self.cur_col - 1);
            self.lines[self.cur_line].remove(b);
            self.cur_col -= 1;
            true
        } else if self.cur_line > 0 {
            let cur = self.lines.remove(self.cur_line);
            self.cur_line -= 1;
            self.cur_col = self.line_len(self.cur_line);
            self.lines[self.cur_line].push_str(&cur);
            true
        } else {
            false
        }
    }

    /// Удаляет символ под курсором; в конце строки — склеивает со следующей.
    /// Возвращает `false`, если удалять нечего (конец файла).
    pub fn delete(&mut self) -> bool {
        if self.cur_col < self.line_len(self.cur_line) {
            let b = self.byte_at(self.cur_line, self.cur_col);
            self.lines[self.cur_line].remove(b);
            true
        } else if self.cur_line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cur_line + 1);
            self.lines[self.cur_line].push_str(&next);
            true
        } else {
            false
        }
    }

    /// Ставит курсор по клику мыши: `row` — строка внутри видимой области,
    /// `disp_col` — экранная колонка от левого края области. Строка — от `voff`,
    /// колонка мапится в индекс символа по unicode-width (широкие CJK — 2 ячейки);
    /// клик за концом строки/текста клампится.
    pub fn click_at(&mut self, row: usize, disp_col: usize) {
        self.cur_line = (self.voff + row).min(self.lines.len().saturating_sub(1));
        let mut w = 0usize;
        let mut col = self.hoff;
        for c in self.lines[self.cur_line].chars().skip(self.hoff) {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if w + cw > disp_col {
                break;
            }
            w += cw;
            col += 1;
        }
        self.cur_col = col.min(self.line_len(self.cur_line));
    }

    /// Восстанавливает позицию курсора из персиста, клампя к текущему содержимому
    /// (файл мог измениться между запусками).
    pub fn restore_cursor(&mut self, line: usize, col: usize) {
        self.cur_line = line.min(self.lines.len().saturating_sub(1));
        self.cur_col = col.min(self.line_len(self.cur_line));
    }

    /// Прокрутка колесом мыши (курсор двигается вместе с видом).
    pub fn scroll(&mut self, delta: isize) {
        if delta < 0 {
            self.move_up((-delta) as usize);
        } else {
            self.move_down(delta as usize);
        }
    }

    /// Подгоняет прокрутку, чтобы курсор был виден в окне `page`×`cols`.
    pub fn ensure_visible(&mut self, page: usize, cols: usize) {
        let page = page.max(1);
        let cols = cols.max(1);
        if self.cur_line < self.voff {
            self.voff = self.cur_line;
        } else if self.cur_line >= self.voff + page {
            self.voff = self.cur_line + 1 - page;
        }
        if self.cur_col < self.hoff {
            self.hoff = self.cur_col;
        } else if self.cur_col >= self.hoff + cols {
            self.hoff = self.cur_col + 1 - cols;
        }
    }
}

/// Вопрос при закрытии редактора с несохранёнными правками.
pub(super) const EDITOR_QUIT_MSG: &str = "File has been modified. Quit without saving?";

impl App {
    /// Открывает файл во встроенном редакторе в активной панели.
    pub(super) fn open_editor(&mut self, path: &std::path::Path) {
        match Editor::load(path) {
            Ok(e) => {
                self.active_panel_mut().editor = Some(e);
                self.panels_dirty = true;
                self.status = EDITOR_HINT.to_string();
            }
            Err(e) => self.status = format!("edit: cannot read {}: {e}", path.display()),
        }
    }

    /// Закрытие без сохранения (`Esc` / `Ctrl+x q`): при несохранённых правках —
    /// сначала диалог подтверждения.
    fn quit_editor(&mut self) {
        let dirty = self.active_panel().editor.as_ref().is_some_and(|e| e.dirty);
        if dirty {
            self.dialog = Some(Dialog::Confirm {
                message: EDITOR_QUIT_MSG.to_string(),
                op: PendingOp::QuitEditor,
            });
        } else {
            self.close_editor();
        }
    }

    /// Закрывает редактор активной панели.
    pub(super) fn close_editor(&mut self) {
        self.active_panel_mut().editor = None;
        self.panels_dirty = true;
        self.status.clear();
        self.reload(); // файл мог измениться — обновляем список (размер/дата)
    }

    /// Явное сохранение (`Ctrl+x s` / `Ctrl+x x`); ошибка — в статус.
    fn editor_save(&mut self) -> bool {
        let res = match self.active_panel_mut().editor.as_mut() {
            Some(e) => {
                let r = e.save();
                if r.is_ok() {
                    e.dirty = false;
                }
                r
            }
            None => return false,
        };
        match res {
            Ok(()) => {
                self.status = "saved".to_string();
                true
            }
            Err(e) => {
                self.status = format!("edit: save failed: {e}");
                false
            }
        }
    }

    pub(super) fn handle_editor_key(&mut self, key: KeyEvent) -> Action {
        // Аккорд `Ctrl+x` в редакторе: s save · x save+close · q close · h help.
        if self.prefix == Prefix::Root {
            self.prefix = Prefix::None;
            self.status.clear();
            match key.code {
                KeyCode::Char('s') => {
                    self.editor_save();
                }
                KeyCode::Char('x') => {
                    if self.editor_save() {
                        self.close_editor();
                    }
                }
                KeyCode::Char('q') => self.quit_editor(),
                KeyCode::Char('h') => self.open_help(),
                _ => {} // прочее (в т.ч. Esc) — отмена аккорда
            }
            return Action::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if key.code == KeyCode::Char('x') {
                self.prefix = Prefix::Root;
                self.status = HINT_EDITOR.to_string();
            }
            return Action::Redraw; // прочие Ctrl-комбинации в тексте не печатаются
        }
        if key.code == KeyCode::Esc {
            self.quit_editor();
            return Action::Redraw;
        }
        let mut modified = false;
        if let Some(ed) = self.active_panel_mut().editor.as_mut() {
            let page = ed.page.max(1);
            match key.code {
                KeyCode::Left => ed.move_left(),
                KeyCode::Right => ed.move_right(),
                KeyCode::Up => ed.move_up(1),
                KeyCode::Down => ed.move_down(1),
                KeyCode::PageUp => ed.move_up(page),
                KeyCode::PageDown => ed.move_down(page),
                KeyCode::Home => ed.cur_col = 0,
                KeyCode::End => ed.cur_col = ed.line_len(ed.cur_line),
                KeyCode::Enter => {
                    ed.insert_newline();
                    modified = true;
                }
                KeyCode::Backspace => modified = ed.backspace(),
                KeyCode::Delete => modified = ed.delete(),
                // Tab — 4 пробела (реальный `\t` ломал бы соответствие колонка↔символ).
                KeyCode::Tab => {
                    for _ in 0..4 {
                        ed.insert_char(' ');
                    }
                    modified = true;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                    ed.insert_char(c);
                    modified = true;
                }
                _ => {}
            }
            if modified {
                ed.dirty = true; // правки — только в буфере, на диск по c-x s / c-x x
            }
            let (p, c) = (ed.page, ed.cols);
            ed.ensure_visible(p, c);
            // Позиция курсора персистится (восстановление после перезапуска).
            self.panels_dirty = true;
        }
        if modified {
            self.status.clear();
        }
        Action::Redraw
    }
}

#[cfg(test)]
#[path = "editor_test.rs"]
mod tests;
