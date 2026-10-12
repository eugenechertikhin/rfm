//! Файловые операции поверх VFS: create/delete/copy/move между локациями
//! (локальная ФС, SFTP, FTP; архив — только источник копирования).
//!
//! Стратегия: в пределах одного бэкенда — «быстрый» путь (`cp`/`mv` по ssh, `RNFR/RNTO`,
//! локальные `fs`); локальное ↔ удалённое — прямая заливка/скачивание; прочее
//! (удалённое ↔ удалённое, архив → удалённое, ftp → тот же ftp) — через временную
//! локальную директорию.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::{archive, ftp, remote_join, sftp};
use crate::ops;

/// Открытая SFTP-сессия: адрес + control-сокет мастер-соединения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SftpConn {
    pub target: sftp::SftpTarget,
    pub sock: PathBuf,
}

/// FTP-«сессия»: адрес + пароль в памяти (`None` — анонимно).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtpConn {
    pub target: ftp::FtpTarget,
    pub password: Option<String>,
}

/// Архив-источник: файл архива и его тип (`name` из конфига — выбирает команду извлечения).
/// `remote` — архив лежит на SFTP-сервере: извлечение идёт там, качается только запись.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveSrc {
    pub file: PathBuf,
    pub kind: String,
    pub remote: Option<SftpConn>,
}

/// Путь в конкретном бэкенде. Удалённые пути — POSIX-строки; путь в архиве —
/// относительно его корня (пусто = корень).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loc {
    Local(PathBuf),
    Sftp(SftpConn, String),
    Ftp(FtpConn, String),
    Archive(ArchiveSrc, String),
}

const READ_ONLY: &str = "archive is read-only";

fn read_only() -> io::Error {
    io::Error::other(READ_ONLY)
}

/// Совпадает ли сервер (пользователь/хост/порт) — путь подключения не важен.
pub fn same_sftp(a: &sftp::SftpTarget, b: &sftp::SftpTarget) -> bool {
    a.user == b.user && a.host == b.host && a.port == b.port
}

pub fn same_ftp(a: &ftp::FtpTarget, b: &ftp::FtpTarget) -> bool {
    a.user == b.user && a.host == b.host && a.port == b.port
}

impl Loc {
    /// Дочерний путь (`name` может содержать `/`; абсолютный `name` — сам по себе).
    pub fn join(&self, name: &str) -> Loc {
        let rj = |cwd: &str| {
            if name.starts_with('/') {
                name.to_string()
            } else {
                remote_join(cwd, name)
            }
        };
        match self {
            Loc::Local(p) => Loc::Local(p.join(name)),
            Loc::Sftp(c, p) => Loc::Sftp(c.clone(), rj(p)),
            Loc::Ftp(c, p) => Loc::Ftp(c.clone(), rj(p)),
            Loc::Archive(a, p) if p.is_empty() => Loc::Archive(a.clone(), name.to_string()),
            Loc::Archive(a, p) => Loc::Archive(a.clone(), format!("{p}/{name}")),
        }
    }

    /// Последний компонент пути (имя записи).
    pub fn name(&self) -> String {
        match self {
            Loc::Local(p) => p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            Loc::Sftp(_, p) | Loc::Ftp(_, p) | Loc::Archive(_, p) => {
                p.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string()
            }
        }
    }

    /// Человекочитаемый путь — для сообщений об ошибках.
    pub fn display(&self) -> String {
        match self {
            Loc::Local(p) => p.display().to_string(),
            Loc::Sftp(c, p) => sftp::url(&c.target, p),
            Loc::Ftp(c, p) => ftp::url(&c.target, p),
            Loc::Archive(a, p) => format!("{}:/{p}", a.file.display()),
        }
    }
}

/// Источник операции: локация + директория ли (нужно FTP/архиву — у них нет `test -d`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub loc: Loc,
    pub is_dir: bool,
}

impl Item {
    /// Локальный элемент (тип берётся с диска).
    #[cfg(test)]
    pub fn local(path: PathBuf) -> Self {
        let is_dir = path.is_dir();
        Self {
            loc: Loc::Local(path),
            is_dir,
        }
    }

    pub fn name(&self) -> String {
        self.loc.name()
    }
}

/// Временная локальная директория; удаляется вместе с содержимым при drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> io::Result<Self> {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("rfm-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p)?;
        Ok(Self(p))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Локальная временная копия файла из любой локации — для view/edit удалённых файлов
/// и записей архива. Временная директория удаляется вместе с копией (drop).
pub struct LocalCopy {
    _tmp: TempDir,
    /// Путь копии (имя файла сохраняется — расширение нужно prettify).
    pub path: PathBuf,
    /// Откуда скачан файл (туда же заливается после правки).
    pub origin: Item,
}

impl LocalCopy {
    /// Скачивает файл во временную директорию (`tag` — часть её имени).
    pub fn fetch(origin: Item, tag: &str) -> io::Result<Self> {
        let tmp = TempDir::new(tag)?;
        let name = origin.name();
        let path = tmp.path().join(if name.is_empty() { "file" } else { &name });
        fetch(&origin, &path)?;
        Ok(Self { _tmp: tmp, path, origin })
    }

    /// Заливает копию обратно в исходную локацию.
    pub fn upload(&self) -> io::Result<()> {
        put(&self.path, &self.origin.loc)
    }

    /// Оставить копию на диске (временная директория не удаляется) — путь к ней.
    pub fn keep(self) -> PathBuf {
        let LocalCopy { _tmp, path, .. } = self;
        std::mem::forget(_tmp);
        path
    }

    /// `(mtime, size)` копии — чтобы понять, менял ли её внешний редактор.
    pub fn stamp(&self) -> Option<(std::time::SystemTime, u64)> {
        let m = std::fs::metadata(&self.path).ok()?;
        Some((m.modified().ok()?, m.len()))
    }
}

/// Создаёт директорию (с недостающими родителями).
pub fn mkdir(loc: &Loc) -> io::Result<()> {
    match loc {
        Loc::Local(p) => ops::create_dir(p),
        Loc::Sftp(c, p) => sftp::mkdir(&c.sock, &c.target, p),
        Loc::Ftp(c, p) => ftp::mkdir(&c.target, c.password.as_deref(), p),
        Loc::Archive(..) => Err(read_only()),
    }
}

/// Удаляет файл/директорию (рекурсивно).
pub fn delete(item: &Item) -> io::Result<()> {
    match &item.loc {
        Loc::Local(p) => ops::delete_path(p),
        Loc::Sftp(c, p) => sftp::remove(&c.sock, &c.target, p),
        Loc::Ftp(c, p) => ftp::remove(&c.target, c.password.as_deref(), p, item.is_dir),
        Loc::Archive(..) => Err(read_only()),
    }
}

/// Существующая директория? (Для выбора «внутрь» / «точный путь» у назначения.)
pub fn is_dir(loc: &Loc) -> bool {
    match loc {
        Loc::Local(p) => p.is_dir(),
        Loc::Sftp(c, p) => sftp::is_dir(&c.sock, &c.target, p),
        Loc::Ftp(c, p) => ftp::is_dir(&c.target, c.password.as_deref(), p),
        Loc::Archive(..) => false,
    }
}

/// Кладёт локальный файл/директорию `local` в точный путь `dest`.
pub fn put(local: &Path, dest: &Loc) -> io::Result<()> {
    match dest {
        Loc::Local(p) => ops::copy_to(local, p),
        Loc::Sftp(c, p) => sftp::upload(&c.sock, &c.target, local, p),
        Loc::Ftp(c, p) => ftp::upload(&c.target, c.password.as_deref(), local, p),
        Loc::Archive(..) => Err(read_only()),
    }
}

/// Достаёт элемент в точный локальный путь `local`.
pub fn fetch(item: &Item, local: &Path) -> io::Result<()> {
    match &item.loc {
        Loc::Local(p) => ops::copy_to(p, local),
        Loc::Sftp(c, p) => sftp::download(&c.sock, &c.target, p, local),
        Loc::Ftp(c, p) => ftp::download(&c.target, c.password.as_deref(), p, item.is_dir, local),
        Loc::Archive(a, member) => match &a.remote {
            Some(c) => fetch_remote_member(c, a, member, item.is_dir, local),
            None => {
                // Извлечение кладёт запись под `tmp/member` — затем переносим в цель.
                let tmp = TempDir::new("extract")?;
                archive::extract(&a.kind, &a.file, member, item.is_dir, tmp.path())?;
                ops::move_to(&tmp.path().join(member), local)
            }
        },
    }
}

/// Запись архива на SFTP-сервере: извлекаем её во временную директорию **на сервере**
/// (`mktemp -d`), качаем только её и чистим временную директорию. Архив целиком не качается.
fn fetch_remote_member(c: &SftpConn, a: &ArchiveSrc, member: &str, is_dir: bool, local: &Path) -> io::Result<()> {
    let file = a.file.to_string_lossy();
    // Тип без команды извлечения — ошибка до каких-либо действий на сервере.
    archive::remote_extract_cmd(&a.kind, &file, member, is_dir, "/tmp")
        .ok_or_else(|| io::Error::other(format!("extract is not supported for {}", a.kind)))?;
    let rtmp = sftp::ssh_run(&c.sock, &c.target, "mktemp -d")?.trim().to_string();
    if rtmp.is_empty() {
        return Err(io::Error::other("mktemp: no directory created"));
    }
    let res = (|| {
        let cmd = archive::remote_extract_cmd(&a.kind, &file, member, is_dir, &rtmp).unwrap_or_default();
        sftp::ssh_run(&c.sock, &c.target, &cmd)?;
        sftp::download(&c.sock, &c.target, &remote_join(&rtmp, member), local)
    })();
    let _ = sftp::remove(&c.sock, &c.target, &rtmp);
    res
}

/// Копирует элемент в точный путь `dest` (любые сочетания бэкендов; архив — только источник).
pub fn copy(item: &Item, dest: &Loc) -> io::Result<()> {
    match (&item.loc, dest) {
        (_, Loc::Archive(..)) => Err(read_only()),
        (Loc::Sftp(a, src), Loc::Sftp(b, dst)) if same_sftp(&a.target, &b.target) => {
            sftp::copy(&a.sock, &a.target, src, dst)
        }
        (Loc::Local(src), _) => put(src, dest),
        (_, Loc::Local(dst)) => fetch(item, dst),
        _ => via_temp(item, dest),
    }
}

/// Копирование через временную локальную директорию (скачать → залить).
fn via_temp(item: &Item, dest: &Loc) -> io::Result<()> {
    let tmp = TempDir::new("xfer")?;
    let name = item.name();
    let local = tmp.path().join(if name.is_empty() { "item" } else { &name });
    fetch(item, &local)?;
    put(&local, dest)
}

/// Перемещает элемент в точный путь `dest`: в пределах бэкенда — переименованием,
/// иначе — копирование + удаление источника. Из архива/в архив — нельзя.
pub fn move_(item: &Item, dest: &Loc) -> io::Result<()> {
    match (&item.loc, dest) {
        (Loc::Archive(..), _) | (_, Loc::Archive(..)) => Err(read_only()),
        (Loc::Local(src), Loc::Local(dst)) => ops::move_to(src, dst),
        (Loc::Sftp(a, src), Loc::Sftp(b, dst)) if same_sftp(&a.target, &b.target) => {
            sftp::rename(&a.sock, &a.target, src, dst)
        }
        (Loc::Ftp(a, src), Loc::Ftp(b, dst)) if same_ftp(&a.target, &b.target) => {
            ftp::rename(&a.target, a.password.as_deref(), src, dst)
        }
        _ => {
            copy(item, dest)?;
            delete(item)
        }
    }
}

#[cfg(test)]
#[path = "transfer_test.rs"]
mod tests;
