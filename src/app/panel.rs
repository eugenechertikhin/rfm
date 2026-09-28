//! Панель: локация VFS, список записей, сортировка и построение дерева.

use super::*;
use ratatui::layout::Rect;

/// Ключ сортировки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Date,
}

/// Состояние сортировки панели.
#[derive(Debug, Clone, Copy)]
pub struct SortState {
    pub key: SortKey,
    pub reverse: bool,
    pub dirs_first: bool,
}

impl Default for SortState {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            reverse: false,
            dirs_first: true,
        }
    }
}

/// Одна панель: локация VFS, список записей, курсор, пометки, сортировка.
pub struct Panel {
    pub path: VfsPath,
    pub entries: Vec<VfsEntry>,
    pub cursor: usize,
    pub state: ListState,
    pub marked: HashSet<String>,
    pub sort: SortState,
    /// Встроенный просмотрщик, открытый в этой панели (если есть).
    pub viewer: Option<Viewer>,
    /// Встроенный редактор, открытый в этой панели (если есть).
    pub editor: Option<Editor>,
    /// Число колонок с файлами (>1 — имена без размеров, навигация `←`/`→`).
    pub columns: usize,
    /// Переопределение вида списка для этой панели (None — брать глобальный).
    pub view_override: Option<FileListView>,
    /// Левый видимый столбец в многоколоночном режиме (горизонтальный скролл).
    pub grid_left: usize,
    /// Число строк на столбец (высота видимой области) — обновляется при рендере.
    pub grid_rows: usize,
    /// Раскрытые узлы дерева (относительные пути от директории панели).
    pub expanded: HashSet<String>,
    /// Текущий список построен как дерево (в tree-режиме `name` = относительный путь).
    pub is_tree: bool,
    /// Внешний прямоугольник панели на экране (для попаданий мышью). Заполняется при рендере.
    pub area: Rect,
}

impl Panel {
    pub fn new(path: VfsPath) -> Self {
        Self {
            path,
            entries: Vec::new(),
            cursor: 0,
            state: ListState::default(),
            marked: HashSet::new(),
            sort: SortState::default(),
            viewer: None,
            editor: None,
            columns: 1,
            view_override: None,
            grid_left: 0,
            grid_rows: 1,
            expanded: HashSet::new(),
            is_tree: false,
            area: Rect::default(),
        }
    }

    /// Перечитывает директорию согласно виду (`flat`/`tree`): сортировка + `..`.
    /// В tree-режиме строит плоский список раскрытых узлов; `name` = относительный путь.
    pub fn reload(&mut self, show_hidden: bool, view: FileListView) -> std::io::Result<()> {
        self.is_tree = view == FileListView::Tree && self.path.local_path().is_some();
        let mut list = if self.is_tree {
            let base = self.path.local_path().unwrap().to_path_buf();
            let mut rows = Vec::new();
            build_tree(&base, "", 0, show_hidden, self.sort, &self.expanded, &mut rows);
            rows
        } else {
            let mut l = self.path.list(show_hidden)?;
            sort_entries(&mut l, self.sort);
            l
        };
        if self.path.can_go_up() {
            list.insert(0, VfsEntry::dotdot());
        }
        let present: HashSet<String> = list.iter().map(|e| e.name.clone()).collect();
        self.marked.retain(|n| present.contains(n));
        self.entries = list;
        if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len().saturating_sub(1);
        }
        Ok(())
    }
}

/// Рекурсивно строит видимое дерево: для раскрытых директорий вставляет их
/// содержимое с увеличением глубины. `name` каждой записи — относительный путь.
fn build_tree(
    base: &std::path::Path,
    rel_prefix: &str,
    depth: usize,
    show_hidden: bool,
    sort: SortState,
    expanded: &HashSet<String>,
    out: &mut Vec<VfsEntry>,
) {
    let dir = if rel_prefix.is_empty() {
        base.to_path_buf()
    } else {
        base.join(rel_prefix)
    };
    let mut items = match crate::vfs::local::list_dir(&dir, show_hidden) {
        Ok(v) => v,
        Err(_) => return,
    };
    sort_entries(&mut items, sort);
    for mut e in items {
        let rel = if rel_prefix.is_empty() {
            e.name.clone()
        } else {
            format!("{rel_prefix}/{}", e.name)
        };
        let is_dir = e.is_dir();
        e.depth = depth;
        e.name = rel.clone();
        out.push(e);
        if is_dir && expanded.contains(&rel) {
            build_tree(base, &rel, depth + 1, show_hidden, sort, expanded, out);
        }
    }
}

/// Устанавливает ключ сортировки; повторный тот же ключ инвертирует порядок.
pub(super) fn toggle_sort_key(sort: &mut SortState, key: SortKey) {
    if sort.key == key {
        sort.reverse = !sort.reverse;
    } else {
        sort.key = key;
        sort.reverse = false;
    }
}

/// Сортирует записи согласно состоянию сортировки.
pub(super) fn sort_entries(list: &mut [VfsEntry], sort: SortState) {
    list.sort_by(|a, b| {
        if sort.dirs_first {
            let d = b.is_dir().cmp(&a.is_dir());
            if d != std::cmp::Ordering::Equal {
                return d;
            }
        }
        let ord = match sort.key {
            // Регистрозависимо: заглавные буквы идут вперёд (ASCII-порядок).
            SortKey::Name => a.name.cmp(&b.name),
            SortKey::Size => a.size.unwrap_or(0).cmp(&b.size.unwrap_or(0)),
            SortKey::Date => a.mtime.cmp(&b.mtime),
        };
        if sort.reverse {
            ord.reverse()
        } else {
            ord
        }
    });
}
