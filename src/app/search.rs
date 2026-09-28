//! Инкрементальный поиск по списку и навигация по истории команд.

use super::*;

impl App {
    pub(super) fn history_prev(&mut self) {
        let recent = self.history.recent();
        if recent.is_empty() {
            return;
        }
        let idx = match self.hist_nav {
            None => 0,
            Some(i) => (i + 1).min(recent.len() - 1),
        };
        self.hist_nav = Some(idx);
        self.cmdline = CmdLine::from_str(&recent[idx]);
    }

    pub(super) fn history_next(&mut self) {
        let recent = self.history.recent();
        match self.hist_nav {
            Some(i) if i > 0 => {
                let ni = i - 1;
                self.hist_nav = Some(ni);
                self.cmdline = CmdLine::from_str(&recent[ni]);
            }
            _ => {
                self.hist_nav = None;
                self.cmdline.clear();
            }
        }
    }

    pub(super) fn open_history_search(&mut self) {
        let results = self.history.ranked("");
        self.dialog = Some(Dialog::HistorySearch {
            input: CmdLine::default(),
            results,
            sel: 0,
        });
    }

    pub(super) fn handle_search_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('s') if ctrl => {
                self.search_next();
                Action::None
            }
            KeyCode::Enter => {
                self.search = None;
                Action::Redraw
            }
            KeyCode::Esc => {
                if let Some(s) = self.search.take() {
                    let max = self.active_panel().entries.len().saturating_sub(1);
                    self.active_panel_mut().cursor = s.origin.min(max);
                }
                Action::Redraw
            }
            KeyCode::Up => {
                self.move_cursor(-1);
                Action::None
            }
            KeyCode::Down => {
                self.move_cursor(1);
                Action::None
            }
            KeyCode::Backspace => {
                if let Some(s) = self.search.as_mut() {
                    s.query.pop();
                }
                self.search_apply();
                Action::Redraw
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                if let Some(s) = self.search.as_mut() {
                    s.query.push(c);
                }
                self.search_apply();
                Action::Redraw
            }
            // Прочие клавиши — выходим из поиска (курсор остаётся).
            _ => {
                self.search = None;
                Action::Redraw
            }
        }
    }

    pub(super) fn search_find(&self, from: usize, q: &str) -> Option<usize> {
        let panel = self.active_panel();
        let n = panel.entries.len();
        if n == 0 {
            return None;
        }
        let ql = q.to_lowercase();
        for k in 0..n {
            let i = (from + k) % n;
            if panel.entries[i].name.to_lowercase().contains(&ql) {
                return Some(i);
            }
        }
        None
    }

    pub(super) fn search_apply(&mut self) {
        let q = match &self.search {
            Some(s) => s.query.clone(),
            None => return,
        };
        if q.is_empty() {
            return;
        }
        if let Some(i) = self.search_find(0, &q) {
            self.active_panel_mut().cursor = i;
        }
    }

    pub(super) fn search_next(&mut self) {
        let q = match &self.search {
            Some(s) => s.query.clone(),
            None => return,
        };
        if q.is_empty() {
            return;
        }
        let start = self.active_panel().cursor + 1;
        if let Some(i) = self.search_find(start, &q) {
            self.active_panel_mut().cursor = i;
        }
    }
}
