//! Модальные диалоги и файловые операции (create/copy/move/delete).

use super::*;
use crate::vfs::transfer::{self, Item, Loc};

/// Действие кнопки диалога (то же, что её Ctrl-шорткат).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtnAct {
    /// `c-y`: выполнить (Copy/Move/OK/Delete/Yes).
    Ok,
    /// `c-s`: через sudo.
    Sudo,
    /// `c-n`: отмена.
    Cancel,
    /// `c-l`: применить ко всем (Replace all).
    All,
}

impl BtnAct {
    fn key(self) -> KeyEvent {
        let c = match self {
            BtnAct::Ok => 'y',
            BtnAct::Sudo => 's',
            BtnAct::Cancel => 'n',
            BtnAct::All => 'l',
        };
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
}

/// Кнопки диалога: подпись и действие. Пусто — диалог без кнопок.
pub fn dialog_buttons(d: &Dialog) -> &'static [(&'static str, BtnAct)] {
    use BtnAct::*;
    match d {
        Dialog::Confirm { op: PendingOp::QuitEditor, .. } => &[("[ Yes (c-y) ]", Ok), ("[ Cancel (c-n) ]", Cancel)],
        Dialog::Confirm { .. } => &[("[ Delete (c-y) ]", Ok), ("[ Sudo (c-s) ]", Sudo), ("[ Cancel (c-n) ]", Cancel)],
        Dialog::Input { op: PendingOp::Copy(_), .. } => &[("[ Copy (c-y) ]", Ok), ("[ Sudo (c-s) ]", Sudo), ("[ Cancel (c-n) ]", Cancel)],
        Dialog::Input { op: PendingOp::Move(_), .. } => &[("[ Move (c-y) ]", Ok), ("[ Sudo (c-s) ]", Sudo), ("[ Cancel (c-n) ]", Cancel)],
        Dialog::Input { op: PendingOp::Select(_) | PendingOp::MkDir(_) | PendingOp::EditorSearch, .. } => &[("[ OK (c-y) ]", Ok), ("[ Cancel (c-n) ]", Cancel)],
        Dialog::Replace { .. } => &[("[ Replace (c-y) ]", Ok), ("[ Replace all (c-l) ]", All), ("[ Cancel (c-n) ]", Cancel)],
        _ => &[],
    }
}

impl App {
    /// Клавиши диалога. `Tab` — фокус на следующую кнопку (по кругу), `Enter` —
    /// нажать кнопку в фокусе (= её Ctrl-шорткат). При открытом автодополнении
    /// `Tab`/`Enter` — его.
    pub(super) fn handle_dialog_key(&mut self, key: KeyEvent) -> Action {
        let buttons = self.dialog.as_ref().map(dialog_buttons).unwrap_or(&[]);
        let mut key = key;
        if !buttons.is_empty() && self.completion.is_none() {
            match key.code {
                KeyCode::Tab if key.modifiers.is_empty() => {
                    self.dialog_btn = (self.dialog_btn + 1) % buttons.len();
                    return Action::Redraw;
                }
                KeyCode::Enter => key = buttons[self.dialog_btn.min(buttons.len() - 1)].1.key(),
                _ => {}
            }
        }
        let action = self.dialog_key(key);
        if self.dialog.is_none() {
            self.dialog_btn = 0; // следующий диалог — с фокусом на первой кнопке
        }
        action
    }

    fn dialog_key(&mut self, key: KeyEvent) -> Action {
        let dialog = self.dialog.take().expect("dialog present");
        match dialog {
            Dialog::Replace { find, repl, field } => self.replace_dialog_key(find, repl, field, key),
            Dialog::Confirm { message, op } => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    // Ctrl+Y/Enter — удалить, Ctrl+S — sudo rm, Ctrl+N/Esc — отмена.
                    KeyCode::Char('y') if ctrl => {
                        self.execute_op(op, None);
                        Action::Redraw
                    }
                    KeyCode::Enter => {
                        self.execute_op(op, None);
                        Action::Redraw
                    }
                    KeyCode::Char('s') if ctrl => match &op {
                        PendingOp::Delete(items) => match self.sudo_delete_cmd(items) {
                            Some(cmd) => {
                                self.active_panel_mut().marked.clear();
                                Action::RunShell(cmd)
                            }
                            None => Action::Redraw,
                        },
                        _ => {
                            self.dialog = Some(Dialog::Confirm { message, op });
                            Action::None
                        }
                    },
                    KeyCode::Char('n') if ctrl => {
                        self.status = "cancelled".to_string();
                        Action::Redraw
                    }
                    KeyCode::Esc => {
                        self.status = "cancelled".to_string();
                        Action::Redraw
                    }
                    // Прочие клавиши игнорируем — диалог остаётся открытым.
                    _ => {
                        self.dialog = Some(Dialog::Confirm { message, op });
                        Action::None
                    }
                }
            }
            Dialog::Input {
                prompt,
                mut input,
                op,
            } => {
                // Автодополнение имён (Shift+Tab) — путь дополняем «сырым» именем (без кавычек).
                if self.completion.is_some() || key.code == KeyCode::BackTab {
                    let base = self.active_panel().path.local_path().map(|p| p.to_path_buf());
                    match crate::app::complete_key(
                        &mut input,
                        &mut self.completion,
                        base.as_deref(),
                        true,
                        false,
                        key,
                    ) {
                        crate::app::CompleteKey::Consumed(msg) => {
                            if let Some(m) = msg {
                                self.status = m;
                            }
                            self.dialog = Some(Dialog::Input { prompt, input, op });
                            return Action::Redraw;
                        }
                        crate::app::CompleteKey::Passthrough => {}
                    }
                }
                match key.code {
                KeyCode::Enter => {
                    let dest = PathBuf::from(input.text());
                    self.execute_op(op, Some(dest));
                    Action::Redraw
                }
                KeyCode::Esc => {
                    self.status = "cancelled".to_string();
                    Action::Redraw
                }
                KeyCode::Backspace => {
                    input.backspace();
                    self.dialog = Some(Dialog::Input { prompt, input, op });
                    Action::None
                }
                KeyCode::Left => {
                    input.left();
                    self.dialog = Some(Dialog::Input { prompt, input, op });
                    Action::None
                }
                KeyCode::Right => {
                    input.right();
                    self.dialog = Some(Dialog::Input { prompt, input, op });
                    Action::None
                }
                // Ctrl-действия (кнопки диалога copy/move) и редактирование строки.
                KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match c {
                        // Действия — закрывают диалог:
                        'n' => {
                            self.status = "cancelled".to_string();
                            return Action::Redraw;
                        }
                        'y' => {
                            let dest = PathBuf::from(input.text());
                            self.execute_op(op, Some(dest));
                            return Action::Redraw;
                        }
                        's' => {
                            if let Some(cmd) = self.sudo_op_cmd(&op, &input.text()) {
                                self.active_panel_mut().marked.clear();
                                return Action::RunShell(cmd);
                            }
                        }
                        // Редактирование строки (как в командной строке):
                        'a' => input.home(),
                        'e' => input.end(),
                        'b' => input.left(),
                        'f' => input.right(),
                        'u' => input.kill_to_start(),
                        'k' => input.kill_to_end(),
                        'w' => input.kill_word(),
                        _ => {}
                    }
                    self.dialog = Some(Dialog::Input { prompt, input, op });
                    Action::None
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    input.insert(c);
                    self.dialog = Some(Dialog::Input { prompt, input, op });
                    Action::None
                }
                _ => {
                    self.dialog = Some(Dialog::Input { prompt, input, op });
                    Action::None
                }
                }
            }
            Dialog::HistorySearch {
                mut input,
                mut results,
                mut sel,
            } => match key.code {
                KeyCode::Enter => {
                    if let Some(cmd) = results.get(sel) {
                        self.cmdline = CmdLine::from_str(cmd);
                        self.hist_nav = None;
                    }
                    Action::Redraw
                }
                KeyCode::Esc => Action::Redraw,
                KeyCode::Up => {
                    sel = sel.saturating_sub(1);
                    self.dialog = Some(Dialog::HistorySearch { input, results, sel });
                    Action::None
                }
                KeyCode::Down => {
                    if sel + 1 < results.len() {
                        sel += 1;
                    }
                    self.dialog = Some(Dialog::HistorySearch { input, results, sel });
                    Action::None
                }
                KeyCode::Backspace => {
                    input.backspace();
                    results = self.history.ranked(&input.text());
                    self.dialog = Some(Dialog::HistorySearch {
                        input,
                        results,
                        sel: 0,
                    });
                    Action::None
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    input.insert(c);
                    results = self.history.ranked(&input.text());
                    self.dialog = Some(Dialog::HistorySearch {
                        input,
                        results,
                        sel: 0,
                    });
                    Action::None
                }
                _ => {
                    self.dialog = Some(Dialog::HistorySearch { input, results, sel });
                    Action::None
                }
            },
        }
    }

    pub(super) fn execute_op(&mut self, op: PendingOp, arg: Option<PathBuf>) {
        let mut errors: Vec<String> = Vec::new();
        let mut done = 0usize;
        let verb: &str;

        match op {
            PendingOp::Delete(items) => {
                verb = "Deleted";
                for it in &items {
                    match transfer::delete(it) {
                        Ok(()) => done += 1,
                        Err(e) => errors.push(format!("{}: {e}", it.loc.display())),
                    }
                }
            }
            PendingOp::Copy(items) => {
                verb = "Copied";
                let Some((dest, into_dir)) = self.resolve_dest(arg, items.len()) else {
                    return;
                };
                for it in &items {
                    // В существующую директорию — копируем внутрь (сохраняя имя);
                    // иначе (единственный источник) — в точный путь (переименование).
                    let d = if into_dir { dest.join(&it.name()) } else { dest.clone() };
                    match transfer::copy(it, &d) {
                        Ok(()) => done += 1,
                        Err(e) => errors.push(format!("{}: {e}", it.loc.display())),
                    }
                }
            }
            PendingOp::Move(items) => {
                verb = "Moved";
                let Some((dest, into_dir)) = self.resolve_dest(arg, items.len()) else {
                    return;
                };
                for it in &items {
                    let d = if into_dir { dest.join(&it.name()) } else { dest.clone() };
                    match transfer::move_(it, &d) {
                        Ok(()) => done += 1,
                        Err(e) => errors.push(format!("{}: {e}", it.loc.display())),
                    }
                }
            }
            PendingOp::MkDir(base) => {
                verb = "Created";
                let name = arg
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let name = name.trim();
                if name.is_empty() {
                    self.status = "empty directory name".to_string();
                    return;
                }
                match transfer::mkdir(&base.join(name)) {
                    Ok(()) => done += 1,
                    Err(e) => errors.push(format!("{name}: {e}")),
                }
            }
            PendingOp::FtpConnect(target) => {
                // «Аргумент» диалога — введённый пароль; открываем FTP-локацию.
                let password = arg.map(|p| p.to_string_lossy().into_owned());
                self.open_ftp(target, password);
                return;
            }
            PendingOp::Select(select) => {
                let mask = arg.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                self.apply_mask(&mask, select);
                return;
            }
            PendingOp::QuitEditor => {
                self.close_editor();
                return;
            }
            PendingOp::EditorSearch => {
                let text = arg.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                self.editor_search_start(&text);
                return;
            }
        }

        self.report_op(verb, done, &errors);
        self.active_panel_mut().marked.clear();
        self.reload_all();
    }

    /// Разбирает введённое назначение в локацию:
    /// - `/abs`, `~`, `~/…` — локальная ФС;
    /// - `sftp://…` / `ftp://…` — уже открытая сессия на тот же сервер (в любой панели);
    ///   анонимный FTP (без логина) — и без открытой сессии;
    /// - прочее (относительный путь) — от текущей директории активной панели, в её бэкенде.
    pub(super) fn parse_dest(&self, s: &str) -> Result<Loc, String> {
        if s.contains("://") {
            if let Some(t) = crate::vfs::sftp::parse_url(s) {
                let conn = self.panels.iter().find_map(|p| p.path.find_sftp(&t));
                return match conn {
                    Some(c) => Ok(Loc::Sftp(c, t.path)),
                    None => Err(format!("no open session: {s}")),
                };
            }
            if let Some(t) = crate::vfs::ftp::parse_url(s) {
                let conn = self.panels.iter().find_map(|p| p.path.find_ftp(&t));
                return match conn {
                    Some(c) => Ok(Loc::Ftp(c, t.path)),
                    None if t.user.is_none() => {
                        let path = t.path.clone();
                        Ok(Loc::Ftp(transfer::FtpConn { target: t, password: None }, path))
                    }
                    None => Err(format!("no open session: {s}")),
                };
            }
            if s.starts_with("http://") || s.starts_with("https://") {
                return Err("http is read-only".to_string());
            }
            return Err(format!("unsupported destination: {s}"));
        }
        if s == "~" {
            return dirs::home_dir().map(Loc::Local).ok_or_else(|| "no home directory".to_string());
        }
        if let Some(rest) = s.strip_prefix("~/") {
            return dirs::home_dir()
                .map(|h| Loc::Local(h.join(rest)))
                .ok_or_else(|| "no home directory".to_string());
        }
        if s.starts_with('/') {
            return Ok(Loc::Local(PathBuf::from(s)));
        }
        self.active_panel().path.cwd_loc().map(|base| base.join(s))
    }

    /// Разрешает назначение copy/move в `(локация, в_директорию)` (см. `parse_dest`).
    /// Существующая директория → копируем/переносим внутрь (`true`). Иначе, если
    /// источник один → это точный целевой путь (переименование, `false`); для
    /// нескольких источников путь-не-директория — ошибка.
    pub(super) fn resolve_dest(&mut self, arg: Option<PathBuf>, count: usize) -> Option<(Loc, bool)> {
        let raw = match arg {
            Some(p) if !p.as_os_str().is_empty() => p.to_string_lossy().into_owned(),
            _ => {
                self.status = "no destination".to_string();
                return None;
            }
        };
        let dest = match self.parse_dest(&raw) {
            Ok(d) => d,
            Err(e) => {
                self.status = e;
                return None;
            }
        };
        if matches!(dest, Loc::Archive(..)) {
            self.status = "archive is read-only".to_string();
            return None;
        }
        if transfer::is_dir(&dest) {
            Some((dest, true))
        } else if count == 1 {
            Some((dest, false))
        } else {
            self.status = format!("destination is not a directory: {}", dest.display());
            None
        }
    }

    pub(super) fn report_op(&mut self, verb: &str, done: usize, errors: &[String]) {
        if errors.is_empty() {
            self.status = format!("{verb} {done} item(s)");
        } else {
            self.status = format!("{verb} {done}, {} error(s): {}", errors.len(), errors[0]);
        }
    }

    /// Локация активной панели, пригодная для записи; иначе — статус и `None`.
    fn writable_cwd(&mut self) -> Option<Loc> {
        match self.active_panel().path.cwd_loc() {
            Ok(Loc::Archive(..)) => {
                self.status = "archive is read-only".to_string();
                None
            }
            Ok(l) => Some(l),
            Err(e) => {
                self.status = e;
                None
            }
        }
    }

    pub(super) fn op_mkdir(&mut self) {
        let Some(base) = self.writable_cwd() else {
            return;
        };
        self.dialog = Some(Dialog::Input {
            prompt: "Create directory:".to_string(),
            input: CmdLine::default(),
            op: PendingOp::MkDir(base),
        });
    }

    pub(super) fn op_delete(&mut self) {
        if self.writable_cwd().is_none() {
            return;
        }
        let t = self.targets();
        if t.is_empty() {
            self.status = "nothing to delete".to_string();
            return;
        }
        self.dialog = Some(Dialog::Confirm {
            message: format!("Delete {} item(s)?", t.len()),
            op: PendingOp::Delete(t),
        });
    }

    pub(super) fn op_copy(&mut self) {
        if let Err(e) = self.active_panel().path.cwd_loc() {
            self.status = e;
            return;
        }
        let t = self.targets();
        if t.is_empty() {
            self.status = "nothing to copy".to_string();
            return;
        }
        let dest = self.dest_default();
        self.dialog = Some(Dialog::Input {
            prompt: format!("Copy {} item(s) to:", t.len()),
            input: CmdLine::from_str(&dest),
            op: PendingOp::Copy(t),
        });
    }

    pub(super) fn op_move(&mut self) {
        if self.writable_cwd().is_none() {
            return;
        }
        let t = self.targets();
        if t.is_empty() {
            self.status = "nothing to move".to_string();
            return;
        }
        let dest = self.dest_default();
        self.dialog = Some(Dialog::Input {
            prompt: format!("Move {} item(s) to:", t.len()),
            input: CmdLine::from_str(&dest),
            op: PendingOp::Move(t),
        });
    }

    /// Элементы операции: помеченные, а без пометок — запись под курсором (`..` — нет).
    pub(super) fn targets(&self) -> Vec<Item> {
        let panel = self.active_panel();
        let Ok(base) = panel.path.cwd_loc() else {
            return Vec::new();
        };
        let item = |name: &str| {
            let is_dir = panel.entries.iter().any(|e| e.name == name && e.is_dir());
            Item {
                loc: base.join(name),
                is_dir,
            }
        };
        if !panel.marked.is_empty() {
            let mut names: Vec<&String> = panel.marked.iter().collect();
            names.sort();
            names.into_iter().map(|n| item(n)).collect()
        } else {
            match panel.entries.get(panel.cursor) {
                Some(e) if e.name != ".." => vec![item(&e.name)],
                _ => Vec::new(),
            }
        }
    }

    /// Префилл назначения: директория соседней панели (удалённая — URL сессии).
    pub(super) fn dest_default(&self) -> String {
        let idx = if self.panels.len() > 1 { (self.active + 1) % self.panels.len() } else { self.active };
        let path = &self.panels[idx].path;
        path.remote_url().unwrap_or_else(|| path.display())
    }
}

#[cfg(test)]
#[path = "dialog_test.rs"]
mod tests;
