//! Локальный бэкенд VFS: чтение директории в `Vec<VfsEntry>`.

use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use super::{EntryKind, VfsEntry};
use crate::users::{group_name, user_name};

/// Читает содержимое директории. Метаданные берём с переходом по симлинкам
/// (broken-симлинки дадут пустые поля). Скрытые файлы (начинающиеся с `.`)
/// пропускаются, если `show_hidden == false`.
pub fn list_dir(dir: &Path, show_hidden: bool) -> io::Result<Vec<VfsEntry>> {
    let mut out = Vec::new();
    for de in fs::read_dir(dir)? {
        let de = de?;
        let name = de.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }

        let file_type = de.file_type()?;
        let (kind, symlink_target, target_is_dir) = if file_type.is_symlink() {
            let target = fs::read_link(de.path())
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
            let tdir = fs::metadata(de.path()).map(|m| m.is_dir()).unwrap_or(false);
            (EntryKind::Symlink, target, tdir)
        } else if file_type.is_dir() {
            (EntryKind::Dir, None, true)
        } else if file_type.is_file() {
            (EntryKind::File, None, false)
        } else {
            (EntryKind::Other, None, false)
        };

        // Метаданные с переходом по симлинкам — для размера/прав/времени.
        let meta = fs::metadata(de.path()).ok();
        let is_file = meta.as_ref().map(|m| m.is_file()).unwrap_or(false);
        let mode = meta.as_ref().map(|m| m.permissions().mode());
        let size = meta.as_ref().filter(|m| m.is_file()).map(|m| m.len());
        let mtime = meta.as_ref().and_then(|m| m.modified().ok());
        let executable = mode.map(|m| m & 0o111 != 0).unwrap_or(false) && is_file;
        let owner = meta.as_ref().map(|m| user_name(m.uid()));
        let group = meta.as_ref().map(|m| group_name(m.gid()));

        out.push(VfsEntry {
            name,
            kind,
            size,
            permissions: mode,
            owner,
            group,
            mtime,
            executable,
            symlink_target,
            target_is_dir,
            depth: 0,
        });
    }
    Ok(out)
}
