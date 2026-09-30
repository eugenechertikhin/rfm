//! Модальные диалоги и файловые операции (create/copy/move/delete).

use super::*;

/// Действие кнопки диалога (то же, что её Ctrl-шорткат).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtnAct {
    /// `c-y`: выполнить (Copy/Move/OK/Delete/Yes).
    Ok,
    /// `c-s`: через sudo.
    Sudo,
    /// `c-n`: отмена.
    Cancel,
}

impl BtnAct {
    fn key(self) -> KeyEvent {
        let c = match self {
            BtnAct::Ok => 'y',
            BtnAct::Sudo => 's',
            BtnAct::Cancel => 'n',
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
        Dialog::Input { op: PendingOp::Select(_) | PendingOp::MkDir(_), .. } => &[("[ OK (c-y) ]", Ok), ("[ Cancel (c-n) ]", Cancel)],
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
                        PendingOp::Delete(paths) => {
                            let mut cmd = String::from("sudo rm -rf --");
                            for p in paths {
                                cmd.push(' ');
                                cmd.push_str(&crate::shell::shell_quote(&p.to_string_lossy()));
                            }
                            self.active_panel_mut().marked.clear();
                            Action::RunShell(cmd)
                        }
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
            PendingOp::Delete(paths) => {
                verb = "Deleted";
                for p in &paths {
                    match ops::delete_path(p) {
                        Ok(()) => done += 1,
                        Err(e) => errors.push(format!("{}: {e}", p.display())),
                    }
                }
            }
            PendingOp::Copy(paths) => {
                verb = "Copied";
                let Some((dest, into_dir)) = self.resolve_dest(arg, paths.len()) else {
                    return;
                };
                for p in &paths {
                    // В существующую директорию — копируем внутрь (сохраняя имя);
                    // иначе (единственный источник) — в точный путь (переименование).
                    let r = if into_dir {
                        ops::copy_into(p, &dest)
                    } else {
                        ops::copy_to(p, &dest)
                    };
                    match r {
                        Ok(()) => done += 1,
                        Err(e) => errors.push(format!("{}: {e}", p.display())),
                    }
                }
            }
            PendingOp::Move(paths) => {
                verb = "Moved";
                let Some((dest, into_dir)) = self.resolve_dest(arg, paths.len()) else {
                    return;
                };
                for p in &paths {
                    let r = if into_dir {
                        ops::move_into(p, &dest)
                    } else {
                        ops::move_to(p, &dest)
                    };
                    match r {
                        Ok(()) => done += 1,
                        Err(e) => errors.push(format!("{}: {e}", p.display())),
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
                match ops::create_dir(&base.join(name)) {
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
        }

        self.report_op(verb, done, &errors);
        self.active_panel_mut().marked.clear();
        self.reload_all();
    }

    /// Разрешает назначение copy/move в `(путь, в_директорию)`.
    /// Относительный путь считается от директории активной панели, `~` — от home.
    /// Существующая директория → копируем/переносим внутрь (`true`). Иначе, если
    /// источник один → это точный целевой путь (переименование, `false`); для
    /// нескольких источников путь-не-директория — ошибка.
    pub(super) fn resolve_dest(&mut self, arg: Option<PathBuf>, count: usize) -> Option<(PathBuf, bool)> {
        let raw = match arg {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => {
                self.status = "no destination".to_string();
                return None;
            }
        };
        // Разрешаем относительно директории активной панели (или home для `~`).
        let s = raw.to_string_lossy();
        let resolved = if s == "~" {
            dirs::home_dir().unwrap_or_else(|| raw.clone())
        } else if let Some(rest) = s.strip_prefix("~/") {
            match dirs::home_dir() {
                Some(h) => h.join(rest),
                None => raw.clone(),
            }
        } else if raw.is_absolute() {
            raw.clone()
        } else {
            match self.active_panel().path.local_path() {
                Some(base) => base.join(&raw),
                None => {
                    self.status = "cannot resolve destination here".to_string();
                    return None;
                }
            }
        };
        if resolved.is_dir() {
            Some((resolved, true))
        } else if count == 1 {
            Some((resolved, false))
        } else {
            self.status = format!("destination is not a directory: {}", resolved.display());
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

    pub(super) fn op_mkdir(&mut self) {
        let base = match self.active_panel().path.local_path() {
            Some(p) => p.to_path_buf(),
            None => {
                self.status = "cannot create directory here".to_string();
                return;
            }
        };
        self.dialog = Some(Dialog::Input {
            prompt: "Create directory:".to_string(),
            input: CmdLine::default(),
            op: PendingOp::MkDir(base),
        });
    }

    /// Команда `sudo cp/mv` для copy/move (запускается на экране shell).
    /// Источники — абсолютные пути целей; `dest` — как введено (относительно cwd
    /// активной панели, где и выполнится команда). `None` — op не поддерживает sudo
    /// или пустой dest.
    pub(super) fn sudo_op_cmd(&self, op: &PendingOp, dest: &str) -> Option<String> {
        if dest.trim().is_empty() {
            return None;
        }
        let (tool, paths) = match op {
            PendingOp::Copy(p) => ("cp -r", p),
            PendingOp::Move(p) => ("mv", p),
            _ => return None,
        };
        let mut cmd = format!("sudo {tool} --");
        for p in paths {
            cmd.push(' ');
            cmd.push_str(&crate::shell::shell_quote(&p.to_string_lossy()));
        }
        cmd.push(' ');
        cmd.push_str(&crate::shell::shell_quote(dest));
        Some(cmd)
    }

    pub(super) fn op_delete(&mut self) {
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

    pub(super) fn targets(&self) -> Vec<PathBuf> {
        let panel = self.active_panel();
        let Some(base) = panel.path.local_path() else {
            return Vec::new();
        };
        if !panel.marked.is_empty() {
            let mut v: Vec<PathBuf> = panel.marked.iter().map(|n| base.join(n)).collect();
            v.sort();
            v
        } else if !panel.entries.is_empty() {
            let e = &panel.entries[panel.cursor];
            if e.name == ".." {
                Vec::new()
            } else {
                vec![base.join(&e.name)]
            }
        } else {
            Vec::new()
        }
    }

    pub(super) fn dest_default(&self) -> String {
        if self.panels.len() > 1 {
            let idx = (self.active + 1) % self.panels.len();
            self.panels[idx].path.display()
        } else {
            self.active_panel().path.display()
        }
    }
}

#[cfg(test)]
#[path = "dialog_test.rs"]
mod tests;
