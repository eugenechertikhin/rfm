//! Встроенный простейший редактор (edit = internal): весь файл в памяти,
//! стрелки двигают курсор по символам (`→` в конце строки — на начало следующей),
//! печатные символы вставляются в позицию курсора. Правки живут в буфере;
//! на диск — только по `Ctrl+x s` (сохранить) / `Ctrl+x x` (сохранить и выйти).
//! `Esc`/`Ctrl+x q` — выход БЕЗ сохранения (несохранённое отбрасывается); при
//! несохранённых правках — сначала диалог подтверждения.
//! Emacs-правка строки (`Ctrl+a/e/b/f/u/k/w`), `Ctrl+y` — удалить строку,
//! `Ctrl+s` — поиск (`Ctrl+n`/`Ctrl+p` — следующее/предыдущее), `Shift+↑/↓` —
//! выделение строк, `Ctrl+x c`/`Ctrl+x m` — копировать/перенести блок к курсору.

use super::*;
use std::fs;
use unicode_width::UnicodeWidthChar;

/// Подсказка в строке статуса при открытом редакторе.
pub(super) const EDITOR_HINT: &str =
    "edit: Esc close · c-s search · c-r replace · Shift+Up/Down select · c-x s save · c-x x save+close";
/// Подсказка аккорда `Ctrl+x` в редакторе.
const HINT_EDITOR: &str = "c-x: s save · x save+close · q close · c copy block · m move block · h help";

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
    /// Выделение строк (`Shift+↑/↓`): (якорь, конец). Движение без Shift его
    /// не снимает — блок копируется/переносится к курсору (`Ctrl+x c` / `Ctrl+x m`).
    pub sel: Option<(usize, usize)>,
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
            sel: None,
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
    pub(super) fn line_len(&self, i: usize) -> usize {
        self.lines.get(i).map(|l| l.chars().count()).unwrap_or(0)
    }

    /// Байтовое смещение символа `col` в строке `line` (для вставки/удаления).
    pub(super) fn byte_at(&self, line: usize, col: usize) -> usize {
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

    /// Удаляет от курсора до начала строки (`Ctrl+u`).
    pub fn kill_to_start(&mut self) -> bool {
        if self.cur_col == 0 {
            return false;
        }
        let b = self.byte_at(self.cur_line, self.cur_col);
        self.lines[self.cur_line].replace_range(..b, "");
        self.cur_col = 0;
        true
    }

    /// Удаляет от курсора до конца строки (`Ctrl+k`).
    pub fn kill_to_end(&mut self) -> bool {
        let b = self.byte_at(self.cur_line, self.cur_col);
        if b == self.lines[self.cur_line].len() {
            return false;
        }
        self.lines[self.cur_line].truncate(b);
        true
    }

    /// Стирает слово слева от курсора в пределах строки (`Ctrl+w`): пробелы, затем непробелы.
    pub fn kill_word(&mut self) -> bool {
        let chars: Vec<char> = self.lines[self.cur_line].chars().collect();
        let mut i = self.cur_col;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        if i == self.cur_col {
            return false;
        }
        let (from, to) = (self.byte_at(self.cur_line, i), self.byte_at(self.cur_line, self.cur_col));
        self.lines[self.cur_line].replace_range(from..to, "");
        self.cur_col = i;
        true
    }

    /// Удаляет текущую строку (`Ctrl+y`); курсор — на следующую (она встаёт на это
    /// место), у последней — на предыдущую. Единственная строка становится пустой.
    pub fn delete_line(&mut self) {
        if self.lines.len() == 1 {
            self.lines[0].clear();
        } else {
            self.lines.remove(self.cur_line);
            self.cur_line = self.cur_line.min(self.lines.len() - 1);
        }
        self.cur_col = self.cur_col.min(self.line_len(self.cur_line));
    }

    /// Выделенные строки (включительно), если выделение есть.
    pub fn sel_range(&self) -> Option<(usize, usize)> {
        self.sel.map(|(a, b)| (a.min(b), a.max(b)))
    }

    /// `Shift+↑/↓`: расширяет выделение до строки курсора после сдвига. Если курсор
    /// ушёл с конца выделения (двигали без Shift) — начинается новое выделение.
    pub fn select_move(&mut self, down: bool) {
        let anchor = match self.sel {
            Some((a, end)) if end == self.cur_line => a,
            _ => self.cur_line,
        };
        if down {
            self.move_down(1);
        } else {
            self.move_up(1);
        }
        self.sel = Some((anchor, self.cur_line));
    }

    /// Копирует выделенные строки перед строкой курсора (`Ctrl+x c`); курсор — на
    /// первую вставленную строку. Выделение снимается.
    pub fn copy_block(&mut self) -> Result<(), &'static str> {
        let (a, b) = self.sel_range().ok_or("no selection")?;
        let block: Vec<String> = self.lines[a..=b].to_vec();
        let at = self.cur_line;
        self.lines.splice(at..at, block);
        self.cur_col = 0;
        self.sel = None;
        Ok(())
    }

    /// Переносит выделенные строки перед строкой курсора (`Ctrl+x m`); курсор внутри
    /// выделения — ошибка. Курсор — на первую перенесённую строку. Выделение снимается.
    pub fn move_block(&mut self) -> Result<(), &'static str> {
        let (a, b) = self.sel_range().ok_or("no selection")?;
        if (a..=b).contains(&self.cur_line) {
            return Err("cursor is inside the selection");
        }
        let block: Vec<String> = self.lines.drain(a..=b).collect();
        let n = block.len();
        let at = if self.cur_line > b { self.cur_line - n } else { self.cur_line };
        self.lines.splice(at..at, block);
        self.cur_line = at;
        self.cur_col = 0;
        self.sel = None;
        Ok(())
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

/// Справка редактора (`Ctrl+x h` внутри редактора) — только его клавиши.
const EDITOR_HELP_TEXT: &str = "\
Move:
  arrows                 move cursor (right at eol wraps to next line)
  c-b / c-f              char back / forward
  c-a / c-e  Home / End  start / end of line
  PgUp / PgDn            page up / down

Edit (buffer in memory):
  printable              insert at cursor
  Enter                  split line
  Backspace / Delete     delete left / under cursor
  Tab                    insert 4 spaces
  c-u / c-k              kill to start / end of line
  c-w                    kill word left
  c-y                    delete line

Search:
  c-s                    search (case-insensitive)
  c-n / c-p              next / previous match
  c-r                    search and replace (one / all)

Block (whole lines):
  Shift+Up/Down          select lines
  c-x c                  copy selection before cursor line
  c-x m                  move selection before cursor line
  Esc                    drop selection

File:
  c-x s                  save
  c-x x                  save and close
  Esc / c-x q            close WITHOUT saving (asks if modified)
  c-x h                  this help

Help: arrows scroll · q/Esc close";

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

    /// Копирование / перенос выделенного блока к курсору (`Ctrl+x c` / `Ctrl+x m`).
    fn editor_block(&mut self, mv: bool) {
        let Some(ed) = self.active_panel_mut().editor.as_mut() else {
            return;
        };
        let res = if mv { ed.move_block() } else { ed.copy_block() };
        match res {
            Ok(()) => {
                ed.dirty = true;
                let (p, c) = (ed.page, ed.cols);
                ed.ensure_visible(p, c);
                self.panels_dirty = true;
            }
            Err(e) => self.status = format!("edit: {e}"),
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
                KeyCode::Char('h') => {
                    let lines = EDITOR_HELP_TEXT.lines().map(|l| l.to_string()).collect();
                    self.help = Some(Viewer::from_memory("Editor help".to_string(), lines));
                }
                KeyCode::Char('c') => self.editor_block(false),
                KeyCode::Char('m') => self.editor_block(true),
                _ => {} // прочее (в т.ч. Esc) — отмена аккорда
            }
            return Action::Redraw;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('x') if ctrl => {
                self.prefix = Prefix::Root;
                self.status = HINT_EDITOR.to_string();
                return Action::Redraw;
            }
            KeyCode::Char('s') if ctrl => {
                self.open_editor_search();
                return Action::Redraw;
            }
            KeyCode::Char('r') if ctrl => {
                self.open_editor_replace();
                return Action::Redraw;
            }
            KeyCode::Char('n') if ctrl => {
                self.editor_find(true, false);
                return Action::Redraw;
            }
            KeyCode::Char('p') if ctrl => {
                self.editor_find(false, false);
                return Action::Redraw;
            }
            KeyCode::Esc => {
                // Esc при выделении — только снять его; иначе — выход.
                match self.active_panel_mut().editor.as_mut() {
                    Some(ed) if ed.sel.is_some() => ed.sel = None,
                    _ => self.quit_editor(),
                }
                return Action::Redraw;
            }
            _ => {}
        }
        let mut modified = false;
        if let Some(ed) = self.active_panel_mut().editor.as_mut() {
            let page = ed.page.max(1);
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                // Правка строки (как в командной строке); прочие Ctrl-комбинации в текст не попадают.
                KeyCode::Char(c) if ctrl => match c {
                    'a' => ed.cur_col = 0,
                    'e' => ed.cur_col = ed.line_len(ed.cur_line),
                    'b' => ed.move_left(),
                    'f' => ed.move_right(),
                    'u' => modified = ed.kill_to_start(),
                    'k' => modified = ed.kill_to_end(),
                    'w' => modified = ed.kill_word(),
                    'y' => {
                        ed.delete_line();
                        modified = true;
                    }
                    _ => {}
                },
                KeyCode::Up if shift => ed.select_move(false),
                KeyCode::Down if shift => ed.select_move(true),
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
                ed.sel = None; // номера строк могли сдвинуться — выделение снимаем
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
