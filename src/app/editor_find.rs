//! Поиск и замена во встроенном редакторе: `Ctrl+s` — поиск (`Ctrl+n`/`Ctrl+p` —
//! следующее/предыдущее), `Ctrl+r` — поиск с заменой (одно вхождение / все).
//! Регистронезависимо; строки поиска/замены живут только в памяти.

use super::*;

/// Длина (в исходных символах) совпадения `pat` (уже в нижнем регистре) с позиции `i`
/// строки `chars`, сравнение регистронезависимое; `None` — нет совпадения.
fn match_at(chars: &[char], i: usize, pat: &[char]) -> Option<usize> {
    let mut k = 0;
    let mut used = 0;
    for c in &chars[i..] {
        if k == pat.len() {
            break;
        }
        for lc in c.to_lowercase() {
            if k >= pat.len() || lc != pat[k] {
                return None;
            }
            k += 1;
        }
        used += 1;
    }
    (k == pat.len()).then_some(used)
}

fn lower(needle: &str) -> Vec<char> {
    needle.chars().flat_map(char::to_lowercase).collect()
}

impl Editor {
    /// Ищет `needle` (регистронезависимо) по кругу и ставит курсор на начало
    /// совпадения. Вперёд — от курсора (`inclusive` — совпадение прямо под курсором
    /// тоже годится), назад — строго до курсора. `false` — совпадений нет.
    pub fn find(&mut self, needle: &str, forward: bool, inclusive: bool) -> bool {
        let pat = lower(needle);
        if pat.is_empty() {
            return false;
        }
        let matches = |line: &str| -> Vec<usize> {
            let chars: Vec<char> = line.chars().collect();
            (0..chars.len()).filter(|&i| match_at(&chars, i, &pat).is_some()).collect()
        };
        let n = self.lines.len();
        let (line, col) = (self.cur_line, self.cur_col);
        // Обходим n+1 строк: текущую — дважды (её часть по другую сторону курсора — в конце круга).
        for step in 0..=n {
            let li = if forward { (line + step) % n } else { (line + n - step % n) % n };
            let ms = matches(&self.lines[li]);
            let hit = if forward {
                ms.into_iter().find(|&c| match step {
                    0 => c > col || (inclusive && c == col),
                    s if s == n => c <= col,
                    _ => true,
                })
            } else {
                ms.into_iter().rev().find(|&c| match step {
                    0 => c < col,
                    s if s == n => c >= col,
                    _ => true,
                })
            };
            if let Some(c) = hit {
                self.cur_line = li;
                self.cur_col = c;
                return true;
            }
        }
        false
    }

    /// Заменяет следующее от курсора вхождение (включая под курсором, по кругу);
    /// курсор — за вставленным текстом. `false` — вхождений нет.
    pub fn replace_one(&mut self, needle: &str, repl: &str) -> bool {
        if !self.find(needle, true, true) {
            return false;
        }
        let chars: Vec<char> = self.lines[self.cur_line].chars().collect();
        let len = match_at(&chars, self.cur_col, &lower(needle)).unwrap_or(0);
        let from = self.byte_at(self.cur_line, self.cur_col);
        let to = self.byte_at(self.cur_line, self.cur_col + len);
        self.lines[self.cur_line].replace_range(from..to, repl);
        self.cur_col += repl.chars().count();
        true
    }

    /// Заменяет все (непересекающиеся, слева направо) вхождения; число замен.
    /// Курсор остаётся на своей строке, колонка клампится.
    pub fn replace_all(&mut self, needle: &str, repl: &str) -> usize {
        let pat = lower(needle);
        if pat.is_empty() {
            return 0;
        }
        let mut count = 0;
        for line in self.lines.iter_mut() {
            let chars: Vec<char> = line.chars().collect();
            let mut out = String::with_capacity(line.len());
            let mut i = 0;
            while i < chars.len() {
                match match_at(&chars, i, &pat) {
                    Some(len) => {
                        out.push_str(repl);
                        i += len;
                        count += 1;
                    }
                    None => {
                        out.push(chars[i]);
                        i += 1;
                    }
                }
            }
            *line = out;
        }
        self.cur_col = self.cur_col.min(self.line_len(self.cur_line));
        count
    }
}

impl App {
    /// Открывает диалог поиска (`Ctrl+s`); в поле — последняя строка поиска.
    pub(super) fn open_editor_search(&mut self) {
        let text = self.editor_search.clone().unwrap_or_default();
        self.dialog = Some(Dialog::Input {
            prompt: "search".to_string(),
            input: CmdLine::from_str(&text),
            op: PendingOp::EditorSearch,
        });
    }

    /// Поиск из диалога: запоминает строку и ищет от курсора вперёд.
    pub(super) fn editor_search_start(&mut self, text: &str) {
        if text.is_empty() {
            self.status = "empty search".to_string();
            return;
        }
        self.editor_search = Some(text.to_string());
        self.editor_find(true, true);
    }

    /// Следующее / предыдущее совпадение (`Ctrl+n` / `Ctrl+p`; `inclusive` — для первого поиска).
    pub(super) fn editor_find(&mut self, forward: bool, inclusive: bool) {
        let Some(text) = self.editor_search.clone() else {
            self.status = "no search (c-s)".to_string();
            return;
        };
        let found = match self.active_panel_mut().editor.as_mut() {
            Some(ed) => {
                let f = ed.find(&text, forward, inclusive);
                let (p, c) = (ed.page, ed.cols);
                ed.ensure_visible(p, c);
                f
            }
            None => return,
        };
        self.panels_dirty = true;
        self.status = if found { String::new() } else { format!("not found: {text}") };
    }

    /// Открывает диалог замены (`Ctrl+r`); поля — последние строки поиска и замены.
    pub(super) fn open_editor_replace(&mut self) {
        self.dialog = Some(Dialog::Replace {
            find: CmdLine::from_str(self.editor_search.as_deref().unwrap_or("")),
            repl: CmdLine::from_str(self.editor_replace.as_deref().unwrap_or("")),
            field: 0,
        });
    }

    /// Клавиши диалога замены. `↑`/`↓` — поле, правка строки — как в командной строке;
    /// `Ctrl+y` — заменить следующее, `Ctrl+l` — заменить все, `Ctrl+n`/`Esc` — отмена.
    /// (`Tab`/`Enter` по кнопкам — в общем `handle_dialog_key`.)
    pub(super) fn replace_dialog_key(
        &mut self,
        mut find: CmdLine,
        mut repl: CmdLine,
        mut field: usize,
        key: KeyEvent,
    ) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') if key.code == KeyCode::Esc || ctrl => {
                self.status = "cancelled".to_string();
                return Action::Redraw;
            }
            KeyCode::Char('y') if ctrl => {
                self.editor_replace_run(&find.text(), &repl.text(), false);
                return Action::Redraw;
            }
            KeyCode::Char('l') if ctrl => {
                self.editor_replace_run(&find.text(), &repl.text(), true);
                return Action::Redraw;
            }
            KeyCode::Up | KeyCode::Down => field = 1 - field.min(1),
            _ => {
                let input = if field == 0 { &mut find } else { &mut repl };
                match key.code {
                    KeyCode::Left => input.left(),
                    KeyCode::Right => input.right(),
                    KeyCode::Home => input.home(),
                    KeyCode::End => input.end(),
                    KeyCode::Backspace => input.backspace(),
                    KeyCode::Char(c) if ctrl => match c {
                        'a' => input.home(),
                        'e' => input.end(),
                        'b' => input.left(),
                        'f' => input.right(),
                        'u' => input.kill_to_start(),
                        'k' => input.kill_to_end(),
                        'w' => input.kill_word(),
                        _ => {}
                    },
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => input.insert(c),
                    _ => {}
                }
            }
        }
        self.dialog = Some(Dialog::Replace { find, repl, field });
        Action::Redraw
    }

    /// Выполняет замену (одно вхождение / все) и запоминает строки поиска/замены.
    fn editor_replace_run(&mut self, find: &str, repl: &str, all: bool) {
        if find.is_empty() {
            self.status = "empty search".to_string();
            return;
        }
        self.editor_search = Some(find.to_string());
        self.editor_replace = Some(repl.to_string());
        let Some(ed) = self.active_panel_mut().editor.as_mut() else {
            return;
        };
        let n = if all { ed.replace_all(find, repl) } else { usize::from(ed.replace_one(find, repl)) };
        if n > 0 {
            ed.dirty = true;
            ed.sel = None; // правка текста снимает выделение
            let (p, c) = (ed.page, ed.cols);
            ed.ensure_visible(p, c);
            self.panels_dirty = true;
        }
        self.status = match n {
            0 => format!("not found: {find}"),
            1 => "replaced 1".to_string(),
            n => format!("replaced {n}"),
        };
    }
}

#[cfg(test)]
#[path = "editor_find_test.rs"]
mod tests;
