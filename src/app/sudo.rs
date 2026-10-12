//! Кнопка `Sudo (c-s)` в диалогах delete/copy/move: команда для экрана shell.
//! Локальные элементы — `sudo rm/cp/mv` локально; элементы на одном SFTP-сервере —
//! та же команда на сервере через `ssh -t` по мастер-сокету (пароль спрашивает `sudo`
//! в терминале). Между хостами, на FTP и в архивах sudo не поддерживается.

use super::*;
use crate::vfs::sftp;
use crate::vfs::transfer::{same_sftp, Item, Loc, SftpConn};

/// Где выполнять sudo-команду.
enum SudoHost {
    Local,
    Sftp(SftpConn),
}

/// Общий хост для всех локаций операции; `Err` — текст статуса, почему sudo нельзя.
fn sudo_host<'a>(locs: impl IntoIterator<Item = &'a Loc>) -> Result<SudoHost, &'static str> {
    let mut host: Option<SudoHost> = None;
    for loc in locs {
        let h = match loc {
            Loc::Local(_) => SudoHost::Local,
            Loc::Sftp(c, _) => SudoHost::Sftp(c.clone()),
            Loc::Ftp(..) => return Err("sudo is not supported for ftp"),
            Loc::Archive(..) => return Err("sudo is not supported for archives"),
        };
        match (&host, &h) {
            (None, _) => host = Some(h),
            (Some(SudoHost::Local), SudoHost::Local) => {}
            (Some(SudoHost::Sftp(a)), SudoHost::Sftp(b)) if same_sftp(&a.target, &b.target) => {}
            _ => return Err("sudo is not supported between hosts"),
        }
    }
    host.ok_or("nothing to do")
}

/// Путь локации как строка (для локальной/удалённой команды).
fn loc_path(loc: &Loc) -> String {
    match loc {
        Loc::Local(p) => p.to_string_lossy().into_owned(),
        Loc::Sftp(_, p) | Loc::Ftp(_, p) | Loc::Archive(_, p) => p.clone(),
    }
}

/// `sudo <tool> -- 'p1' 'p2' …` — локально или на сервере (`ssh -t …`).
fn build(host: &SudoHost, tool: &str, paths: &[String]) -> String {
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let cmd = sftp::remote_cmd(&format!("sudo {tool}"), &refs);
    match host {
        SudoHost::Local => cmd,
        SudoHost::Sftp(c) => sftp::tty_cmd(&c.sock, &c.target, &cmd),
    }
}

impl App {
    /// Команда `sudo rm -rf` для delete. `None` — sudo нельзя (статус — почему).
    pub(super) fn sudo_delete_cmd(&mut self, items: &[Item]) -> Option<String> {
        match sudo_host(items.iter().map(|it| &it.loc)) {
            Ok(host) => {
                let paths: Vec<String> = items.iter().map(|it| loc_path(&it.loc)).collect();
                Some(build(&host, "rm -rf", &paths))
            }
            Err(e) => {
                self.status = e.to_string();
                None
            }
        }
    }

    /// Команда `sudo cp -r`/`sudo mv` для copy/move. Локально `dest` — как введено
    /// (относительно cwd активной панели, где и выполнится команда); на SFTP — путь
    /// на сервере (относительный — от директории панели). `None` — op не поддерживает
    /// sudo, пустой dest или источники/назначение на разных хостах (статус — почему).
    pub(super) fn sudo_op_cmd(&mut self, op: &PendingOp, dest: &str) -> Option<String> {
        if dest.trim().is_empty() {
            return None;
        }
        let (tool, items) = match op {
            PendingOp::Copy(p) => ("cp -r", p),
            PendingOp::Move(p) => ("mv", p),
            _ => return None,
        };
        // Локальная панель и не-URL — dest как введён (поведение локального sudo).
        let dest_loc = if self.active_panel().path.local_path().is_some() && !dest.contains("://") {
            Loc::Local(PathBuf::from(dest))
        } else {
            match self.parse_dest(dest) {
                Ok(l) => l,
                Err(e) => {
                    self.status = e;
                    return None;
                }
            }
        };
        match sudo_host(items.iter().map(|it| &it.loc).chain(std::iter::once(&dest_loc))) {
            Ok(host) => {
                let mut paths: Vec<String> = items.iter().map(|it| loc_path(&it.loc)).collect();
                paths.push(loc_path(&dest_loc));
                Some(build(&host, tool, &paths))
            }
            Err(e) => {
                self.status = e.to_string();
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "sudo_test.rs"]
mod tests;
