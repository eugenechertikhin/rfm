//! Файловые операции над локальной ФС: удаление, копирование, перемещение.
//! Рекурсивно для директорий. 
//! 
use std::fs;
use std::io;
use std::path::Path;

/// Создаёт директорию (вместе с недостающими родителями).
pub fn create_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// Удаляет файл или директорию (рекурсивно).
pub fn delete_path(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Копирует `src` внутрь директории `dest_dir`, сохраняя имя.
pub fn copy_into(src: &Path, dest_dir: &Path) -> io::Result<()> {
    let name = src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "source has no file name")
    })?;
    copy_to(src, &dest_dir.join(name))
}

/// Копирует `src` в точный путь `dest` (переименование при копировании).
pub fn copy_to(src: &Path, dest: &Path) -> io::Result<()> {
    copy_recursive(src, dest)
}

fn copy_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dest)?;
        Ok(())
    }
}

/// Перемещает `src` внутрь `dest_dir`, сохраняя имя.
pub fn move_into(src: &Path, dest_dir: &Path) -> io::Result<()> {
    let name = src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "source has no file name")
    })?;
    move_to(src, &dest_dir.join(name))
}

/// Перемещает `src` в точный путь `dest` (переименование). Пытается `rename`;
/// при неудаче (напр. другой раздел) — копирование с последующим удалением.
pub fn move_to(src: &Path, dest: &Path) -> io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_recursive(src, dest)?;
            delete_path(src)
        }
    }
}

#[cfg(test)]
#[path = "ops_test.rs"]
mod tests;
