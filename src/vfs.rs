//! VFS — единый интерфейс к «локациям» (см. устав).

pub mod archive;
pub mod ftp;
pub mod http;
pub mod local;
pub mod sftp;

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use archive::ArchiveItem;

/// Тип записи в списке.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
    Other,
}

/// Единая запись файла для всех источников. Поля, которые источник не поддерживает, — `None`.
#[derive(Debug, Clone)]
pub struct VfsEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub permissions: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mtime: Option<SystemTime>,
    pub executable: bool,
    pub symlink_target: Option<String>,
    /// Для симлинков: указывает ли ссылка на директорию.
    pub target_is_dir: bool,
    /// Глубина в дереве (0 — верхний уровень); в flat всегда 0.
    /// В tree-режиме `name` хранит относительный путь от директории панели.
    pub depth: usize,
}

impl VfsEntry {
    /// Синтетическая запись перехода на уровень вверх.
    pub fn dotdot() -> Self {
        Self {
            name: "..".to_string(),
            kind: EntryKind::Dir,
            size: None,
            permissions: None,
            owner: None,
            group: None,
            mtime: None,
            executable: false,
            symlink_target: None,
            target_is_dir: true,
            depth: 0,
        }
    }

    /// Директория (или симлинк на директорию).
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir || (self.kind == EntryKind::Symlink && self.target_is_dir)
    }
}

/// Слой архива: плоский список записей + текущий подпуть внутри архива.
#[derive(Debug, Clone)]
struct ArchiveLayer {
    /// Имя файла архива — заголовок «директории» и цель курсора при выходе.
    label: String,
    /// Полный плоский список записей архива (читается один раз).
    items: Vec<ArchiveItem>,
    /// Текущий подпуть внутри архива (пусто = корень).
    cwd: String,
}

/// Слой SFTP: параметры соединения + control-сокет + корень сессии и текущий путь.
/// Живого соединения нет — держим путь сокета (мастер `ssh -M` работает в фоне).
#[derive(Debug, Clone)]
struct SftpLayer {
    target: sftp::SftpTarget,
    sock: PathBuf,
    /// Путь подключения — «корень» сессии; `..` на нём снимает слой (дисконнект).
    root: String,
    /// Текущий удалённый путь.
    cwd: String,
}

/// Слой FTP: параметры + пароль в памяти (persistent-соединения у FTP нет) + путь.
#[derive(Debug, Clone)]
struct FtpLayer {
    target: ftp::FtpTarget,
    /// Пароль в памяти (спрошен один раз); `None` — анонимный доступ.
    password: Option<String>,
    root: String,
    cwd: String,
}

/// Слой HTTP: origin (`scheme://host`) + путь. Директории — из HTML-автоиндекса.
#[derive(Debug, Clone)]
struct HttpLayer {
    origin: String,
    root: String,
    cwd: String,
}

/// Один слой пути: `Local` — локальная ФС, `Archive` — архив, `Sftp`/`Ftp`/`Http` — удалённые.
#[derive(Debug, Clone)]
enum Layer {
    Local(PathBuf),
    Archive(ArchiveLayer),
    Sftp(SftpLayer),
    Ftp(FtpLayer),
    Http(HttpLayer),
}

/// POSIX-склейка удалённого пути с именем поддиректории (корень `/` — особый случай).
fn remote_join(cwd: &str, name: &str) -> String {
    if cwd == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), name)
    }
}

/// Путь в VFS — стек слоёв. Вход в архив/на сервер будет push’ить слой, выход —
/// pop’ить (см. устав). Пока стек всегда содержит один `Local`.
#[derive(Debug, Clone)]
pub struct VfsPath {
    stack: Vec<Layer>,
}

impl VfsPath {
    pub fn local(path: PathBuf) -> Self {
        Self {
            stack: vec![Layer::Local(path)],
        }
    }

    fn top(&self) -> &Layer {
        self.stack.last().expect("stack never empty")
    }

    fn top_mut(&mut self) -> &mut Layer {
        self.stack.last_mut().expect("stack never empty")
    }

    /// Отображаемый путь для заголовка панели.
    pub fn display(&self) -> String {
        match self.top() {
            Layer::Local(p) => p.display().to_string(),
            Layer::Archive(a) => format!("{}:/{}", a.label, a.cwd),
            Layer::Sftp(s) => format!("{}:{}", sftp::user_host(&s.target), s.cwd),
            Layer::Ftp(f) => format!("ftp://{}{}", ftp::user_host(&f.target), f.cwd),
            Layer::Http(h) => format!("{}{}", h.origin, h.cwd),
        }
    }

    /// Абсолютный локальный путь, если верхний слой локальный.
    pub fn local_path(&self) -> Option<&Path> {
        match self.top() {
            Layer::Local(p) => Some(p.as_path()),
            _ => None,
        }
    }

    /// Ближайший снизу локальный путь (директория, из которой открыт архив/сессия).
    /// Нужен для `$SHELL -c` и персиста, когда верхний слой не локальный.
    pub fn base_local_path(&self) -> Option<&Path> {
        self.stack.iter().rev().find_map(|l| match l {
            Layer::Local(p) => Some(p.as_path()),
            _ => None,
        })
    }

    /// Список записей текущей локации.
    pub fn list(&self, show_hidden: bool) -> io::Result<Vec<VfsEntry>> {
        match self.top() {
            Layer::Local(p) => local::list_dir(p, show_hidden),
            Layer::Archive(a) => Ok(archive::children(&a.items, &a.cwd)),
            Layer::Sftp(s) => sftp::list(&s.sock, &s.target, &s.cwd),
            Layer::Ftp(f) => ftp::list(&f.target, f.password.as_deref(), &f.cwd),
            Layer::Http(h) => http::list(&h.origin, &h.cwd),
        }
    }

    /// Можно ли подняться выше (есть родитель локально или есть вложенные слои).
    pub fn can_go_up(&self) -> bool {
        match self.top() {
            Layer::Local(p) => p.parent().is_some() || self.stack.len() > 1,
            // Из архива/удалённой сессии всегда можно вверх: либо внутрь, либо снять слой.
            _ => true,
        }
    }

    /// Спуститься в поддиректорию текущего слоя.
    pub fn enter_dir(&mut self, name: &str) {
        match self.top_mut() {
            Layer::Local(p) => p.push(name),
            Layer::Archive(a) => {
                if a.cwd.is_empty() {
                    a.cwd = name.to_string();
                } else {
                    a.cwd = format!("{}/{}", a.cwd, name);
                }
            }
            Layer::Sftp(s) => s.cwd = remote_join(&s.cwd, name),
            Layer::Ftp(f) => f.cwd = remote_join(&f.cwd, name),
            Layer::Http(h) => h.cwd = remote_join(&h.cwd, name),
        }
    }

    /// Push слоя архива (вход по `Enter`/`cd`). `label` — имя файла архива.
    pub fn push_archive(&mut self, label: String, items: Vec<ArchiveItem>) {
        self.stack.push(Layer::Archive(ArchiveLayer {
            label,
            items,
            cwd: String::new(),
        }));
    }

    /// Push слоя SFTP (вход по `cd sftp://…`). `root` — путь подключения (корень сессии).
    pub fn push_sftp(&mut self, target: sftp::SftpTarget, sock: PathBuf, root: String) {
        self.stack.push(Layer::Sftp(SftpLayer {
            target,
            sock,
            cwd: root.clone(),
            root,
        }));
    }

    /// Push слоя FTP (вход по `cd ftp://…`). `password` — из диалога (или `None` — анонимно).
    pub fn push_ftp(&mut self, target: ftp::FtpTarget, password: Option<String>, root: String) {
        self.stack.push(Layer::Ftp(FtpLayer {
            target,
            password,
            cwd: root.clone(),
            root,
        }));
    }

    /// Push слоя HTTP (вход по `cd http(s)://…`). `origin` = `scheme://host`.
    pub fn push_http(&mut self, origin: String, root: String) {
        self.stack.push(Layer::Http(HttpLayer {
            origin,
            cwd: root.clone(),
            root,
        }));
    }

    /// Закрывает мастер-соединения SFTP во всех слоях (при выходе/закрытии панели).
    pub fn close_remote(&self) {
        for l in &self.stack {
            if let Layer::Sftp(s) = l {
                sftp::exit_master(&s.sock, &s.target);
            }
        }
    }

    /// Подняться на уровень вверх. Возвращает имя дочернего элемента, откуда вышли
    /// (для установки курсора): имя покинутой директории или имя архива при снятии слоя.
    pub fn go_up(&mut self) -> Option<String> {
        // Тип верхнего слоя определяем заранее, чтобы при необходимости снять слой.
        enum Top {
            Local,
            ArchiveSub,
            ArchiveRoot,
            SftpSub,
            SftpRoot,
            /// ftp/http: подпуть (навигация вверх) и корень сессии (снять слой, без teardown).
            RemoteSub,
            RemoteRoot,
        }
        let top = match self.top() {
            Layer::Local(_) => Top::Local,
            Layer::Archive(a) if a.cwd.is_empty() => Top::ArchiveRoot,
            Layer::Archive(_) => Top::ArchiveSub,
            Layer::Sftp(s) if s.cwd == s.root => Top::SftpRoot,
            Layer::Sftp(_) => Top::SftpSub,
            Layer::Ftp(f) if f.cwd == f.root => Top::RemoteRoot,
            Layer::Ftp(_) => Top::RemoteSub,
            Layer::Http(h) if h.cwd == h.root => Top::RemoteRoot,
            Layer::Http(_) => Top::RemoteSub,
        };
        match top {
            Top::Local => {
                if let Layer::Local(p) = self.top_mut() {
                    let leaf = p.file_name().map(|s| s.to_string_lossy().into_owned());
                    if p.pop() {
                        return leaf;
                    }
                }
                None
            }
            Top::ArchiveSub => {
                if let Layer::Archive(a) = self.top_mut() {
                    if let Some(pos) = a.cwd.rfind('/') {
                        let leaf = a.cwd[pos + 1..].to_string();
                        a.cwd.truncate(pos);
                        return Some(leaf);
                    }
                    return Some(std::mem::take(&mut a.cwd));
                }
                None
            }
            Top::ArchiveRoot => match self.stack.pop() {
                Some(Layer::Archive(a)) => Some(a.label),
                _ => None,
            },
            Top::SftpSub => {
                if let Layer::Sftp(s) = self.top_mut() {
                    if let Some(pos) = s.cwd.rfind('/') {
                        let leaf = s.cwd[pos + 1..].to_string();
                        // корень удалённой ФС — минимум "/"
                        s.cwd.truncate(pos.max(1));
                        return Some(leaf);
                    }
                }
                None
            }
            // Корень сессии — рвём соединение и снимаем слой.
            Top::SftpRoot => match self.stack.pop() {
                Some(Layer::Sftp(s)) => {
                    sftp::exit_master(&s.sock, &s.target);
                    None
                }
                _ => None,
            },
            Top::RemoteSub => {
                let cwd = match self.top_mut() {
                    Layer::Ftp(f) => &mut f.cwd,
                    Layer::Http(h) => &mut h.cwd,
                    _ => return None,
                };
                if let Some(pos) = cwd.rfind('/') {
                    let leaf = cwd[pos + 1..].to_string();
                    cwd.truncate(pos.max(1)); // минимум "/"
                    return Some(leaf);
                }
                None
            }
            // Корень ftp/http-сессии — соединения нет, просто снимаем слой.
            Top::RemoteRoot => {
                self.stack.pop();
                None
            }
        }
    }

    /// Перейти к конкретной локальной директории (`cd` по абсолютному/относительному пути):
    /// сбрасывает стек в один локальный слой (в т.ч. выходя из архива).
    pub fn set_local(&mut self, path: PathBuf) {
        self.stack = vec![Layer::Local(path)];
    }

    /// Push слоя архива из готового набора записей (для тестов навигации).
    #[cfg(test)]
    pub(crate) fn push_archive_items(&mut self, label: &str, items: Vec<ArchiveItem>) {
        self.push_archive(label.to_string(), items);
    }
}

#[cfg(test)]
#[path = "vfs_test.rs"]
mod tests;
