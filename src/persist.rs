//! Сохранение/восстановление состояния панелей (количество, открытые директории,
//! открытый просмотрщик/редактор) в файле `panel` в data-dir по XDG:
//! `$XDG_DATA_HOME/rfm/panel` → `~/.local/share/rfm/panel`.
//!
//! Формат: первая строка — индекс активной панели; далее по строке на панель
//! (поля через `\t`, пустые допустимы):
//! `dir \t columns \t view \t viewer_path \t voff \t editor_path \t cur_line \t cur_col`.

use crate::config::FileListView;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Состояние одной панели для персиста.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelState {
    pub dir: PathBuf,
    pub columns: usize,
    pub view_override: Option<FileListView>,
    /// Открытый просмотрщик: путь к файлу и вертикальное смещение.
    pub viewer: Option<(PathBuf, usize)>,
    /// Открытый редактор: путь к файлу и курсор (строка, колонка).
    pub editor: Option<(PathBuf, usize, usize)>,
}

/// Путь к файлу состояния панелей.
pub fn data_file() -> Option<PathBuf> {
    if let Ok(x) = env::var("XDG_DATA_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("rfm/panel"));
        }
    }
    dirs::home_dir().map(|h| h.join(".local/share/rfm/panel"))
}

/// Сохраняет состояние (best effort).
pub fn save(active: usize, panels: &[PanelState]) {
    if let Some(path) = data_file() {
        let _ = save_to(&path, active, panels);
    }
}

/// Загружает состояние, если файл есть и валиден.
pub fn load() -> Option<(usize, Vec<PanelState>)> {
    load_from(&data_file()?)
}

fn save_to(path: &Path, active: usize, panels: &[PanelState]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut s = format!("{active}\n");
    for p in panels {
        let view = match p.view_override {
            Some(FileListView::Flat) => "flat",
            Some(FileListView::Tree) => "tree",
            None => "",
        };
        let (vp, voff) = match &p.viewer {
            Some((vp, off)) => (vp.to_string_lossy().into_owned(), off.to_string()),
            None => (String::new(), String::new()),
        };
        let (ep, eline, ecol) = match &p.editor {
            Some((ep, l, c)) => (ep.to_string_lossy().into_owned(), l.to_string(), c.to_string()),
            None => (String::new(), String::new(), String::new()),
        };
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            p.dir.to_string_lossy(),
            p.columns,
            view,
            vp,
            voff,
            ep,
            eline,
            ecol
        ));
    }
    fs::write(path, s)
}

fn load_from(path: &Path) -> Option<(usize, Vec<PanelState>)> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let active: usize = lines.next()?.trim().parse().ok()?;
    let mut panels = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let dir = PathBuf::from(parts.first().copied().unwrap_or(""));
        if !dir.is_dir() {
            continue;
        }
        let columns = parts
            .get(1)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(1)
            .max(1);
        let view_override = match parts.get(2).copied() {
            Some("flat") => Some(FileListView::Flat),
            Some("tree") => Some(FileListView::Tree),
            _ => None,
        };
        let viewer = match parts.get(3).copied() {
            Some(vp) if !vp.is_empty() => {
                let vpath = PathBuf::from(vp);
                if vpath.is_file() {
                    let off = parts.get(4).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                    Some((vpath, off))
                } else {
                    None
                }
            }
            _ => None,
        };
        let editor = match parts.get(5).copied() {
            Some(ep) if !ep.is_empty() => {
                let epath = PathBuf::from(ep);
                if epath.is_file() {
                    let line = parts.get(6).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                    let col = parts.get(7).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                    Some((epath, line, col))
                } else {
                    None
                }
            }
            _ => None,
        };
        panels.push(PanelState {
            dir,
            columns,
            view_override,
            viewer,
            editor,
        });
    }
    if panels.is_empty() {
        return None;
    }
    Some((active.min(panels.len() - 1), panels))
}

#[cfg(test)]
#[path = "persist_test.rs"]
mod tests;
