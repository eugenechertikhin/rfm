//! Рендер панели со списком файлов: плоский, многоколоночный и дерево.

use super::*;

/// Человекочитаемый размер.
pub(super) fn human_size(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut f = n as f64;
    let mut i = 0;
    while f >= 1024.0 && i < UNITS.len() - 1 {
        f /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n}B")
    } else {
        format!("{f:.1}{}", UNITS[i])
    }
}

/// Форматирует запись в строку фиксированной ширины: имя слева, размер справа.
pub(super) fn format_entry(e: &VfsEntry, width: usize) -> String {
    let mut name = e.name.clone();
    if e.is_dir() && e.name != ".." {
        name.push('/');
    }
    let size = e.size.map(human_size).unwrap_or_default();
    let size_w = UnicodeWidthStr::width(size.as_str());
    let name_area = width.saturating_sub(size_w + 1).max(1);
    let name_t = truncate_width(&name, name_area);
    let name_w = UnicodeWidthStr::width(name_t.as_str());
    let pad = width.saturating_sub(name_w + size_w);
    format!("{name_t}{}{size}", " ".repeat(pad))
}

/// Строка прав в стиле `ls -l`: тип + `rwxrwxrwx`, где setuid/setgid → `s`/`S`
/// в триадах владельца/группы, sticky → `t`/`T` в триаде остальных.
pub(super) fn format_mode(e: &VfsEntry, mode: Option<u32>) -> String {
    let t = match e.kind {
        EntryKind::Dir => 'd',
        EntryKind::Symlink => 'l',
        EntryKind::File => '-',
        EntryKind::Other => '?',
    };
    let Some(m) = mode else {
        return format!("{t}---------");
    };
    // Триада rwx с учётом спец-бита: special=setuid/setgid/sticky, spec_ch='s'/'t'.
    let triad = |shift: u32, special: bool, spec_lower: char| -> String {
        let bits = (m >> shift) & 0o7;
        let r = if bits & 0o4 != 0 { 'r' } else { '-' };
        let w = if bits & 0o2 != 0 { 'w' } else { '-' };
        let x_set = bits & 0o1 != 0;
        let x = match (special, x_set) {
            (true, true) => spec_lower,
            (true, false) => spec_lower.to_ascii_uppercase(),
            (false, true) => 'x',
            (false, false) => '-',
        };
        format!("{r}{w}{x}")
    };
    let user = triad(6, m & 0o4000 != 0, 's');
    let group = triad(3, m & 0o2000 != 0, 's');
    let other = triad(0, m & 0o1000 != 0, 't');
    format!("{t}{user}{group}{other}")
}

/// Строка-инфо по записи под курсором: имя (слева, обрезается) + размер,
/// владелец:группа и права (справа, всегда видны). Заполняется до `width`.
pub(super) fn file_info_line(e: &VfsEntry, width: usize) -> String {
    let size = e.size.map(human_size).unwrap_or_else(|| "-".to_string());
    let owner = e.owner.clone().unwrap_or_else(|| "-".to_string());
    let group = e.group.clone().unwrap_or_else(|| "-".to_string());
    let perms = format_mode(e, e.permissions);
    let right = format!("{size}  {owner}:{group}  {perms}");
    let right_w = UnicodeWidthStr::width(right.as_str());
    // Имя занимает остаток слева (минимум 1), метаданные прижаты вправо.
    let name_area = width.saturating_sub(right_w + 2).max(1);
    let name = truncate_width(&e.name, name_area);
    let name_w = UnicodeWidthStr::width(name.as_str());
    let pad = width.saturating_sub(name_w + right_w).max(1);
    format!("{name}{}{right}", " ".repeat(pad))
}

/// Делит внутреннюю область панели на область списка, строку-разделитель и строку-инфо.
/// При высоте >= 3 отводятся обе нижние строки (разделитель + инфо); при высоте 2 —
/// только инфо (без разделителя); иначе всё занимает список.
pub(super) fn split_info(inner: Rect) -> (Rect, Option<Rect>, Option<Rect>) {
    let h = inner.height;
    if h >= 3 {
        let list = Rect::new(inner.x, inner.y, inner.width, h - 2);
        let sep = Rect::new(inner.x, inner.y + h - 2, inner.width, 1);
        let info = Rect::new(inner.x, inner.y + h - 1, inner.width, 1);
        (list, Some(sep), Some(info))
    } else if h == 2 {
        let list = Rect::new(inner.x, inner.y, inner.width, 1);
        let info = Rect::new(inner.x, inner.y + 1, inner.width, 1);
        (list, None, Some(info))
    } else {
        (inner, None, None)
    }
}

/// Стиль рамки панели (совпадает с `panel_block`): активная — жирная.
fn border_style(theme: &Theme, active: bool) -> Style {
    if active {
        Style::default().bg(theme.bg).fg(theme.fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(theme.bg).fg(Color::DarkGray)
    }
}

/// Рисует нижний футер панели: горизонтальную черту-разделитель, подключённую к
/// рамке (`├───┤`), и строку-инфо о записи под курсором этой панели.
pub(super) fn render_panel_footer(
    f: &mut Frame,
    sep: Option<Rect>,
    info: Rect,
    panel: &Panel,
    theme: &Theme,
    active: bool,
) {
    if let Some(sep) = sep {
        let bs = border_style(theme, active);
        let line = "─".repeat(sep.width as usize);
        f.render_widget(Paragraph::new(line).style(bs), sep);
        // Тройники на левой/правой стойках рамки.
        f.render_widget(Paragraph::new("├").style(bs), Rect::new(sep.x - 1, sep.y, 1, 1));
        f.render_widget(Paragraph::new("┤").style(bs), Rect::new(sep.x + sep.width, sep.y, 1, 1));
    }
    // Инфо-строка — фоном/цветом панели.
    let w = info.width as usize;
    let text = if panel.entries.is_empty() {
        String::new()
    } else {
        let idx = panel.cursor.min(panel.entries.len() - 1);
        file_info_line(&panel.entries[idx], w)
    };
    let line = format!("{:<width$}", text, width = w);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.bg).fg(theme.fg)),
        info,
    );
}

/// Общий блок панели: рамка + заголовок; возвращает внутреннюю область.
pub(super) fn panel_block(area: Rect, path: &str, theme: &Theme, active: bool) -> (Block<'static>, Rect) {
    let base_style = Style::default().bg(theme.bg).fg(theme.fg);
    let border_style = if active {
        Style::default().bg(theme.bg).fg(theme.fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(theme.bg).fg(Color::DarkGray)
    };
    let title = if active {
        format!("[ {path} ]")
    } else {
        format!("  {path}  ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .style(base_style);
    let inner = block.inner(area);
    (block, inner)
}

/// Рисует панель в многоколоночном режиме (column-major: заполнение по столбцам сверху вниз).
/// Имена без размеров; `←`/`→` — между столбцами. Скролл — горизонтальный, по столбцам.
pub(super) fn render_panel_grid(f: &mut Frame, area: Rect, panel: &mut Panel, theme: &Theme, active: bool) {
    let (block, inner) = panel_block(area, &panel.path.display().to_string(), theme, active);
    f.render_widget(block, area);
    let (list_area, sep_area, info_area) = split_info(inner);
    let cols = panel.columns.max(1);
    let rows = list_area.height as usize;
    if rows == 0 || list_area.width == 0 {
        return;
    }
    // Высота столбца = видимые строки (нужно навигации ←/→).
    panel.grid_rows = rows;
    let col_w = (list_area.width as usize / cols).max(1);
    let total = panel.entries.len();

    // Горизонтальный скролл: держим столбец курсора в видимой области.
    let cursor_col = panel.cursor / rows;
    if cursor_col < panel.grid_left {
        panel.grid_left = cursor_col;
    } else if cursor_col >= panel.grid_left + cols {
        panel.grid_left = cursor_col + 1 - cols;
    }

    for sc in 0..cols {
        let col = panel.grid_left + sc; // фактический столбец
        for r in 0..rows {
            let idx = col * rows + r; // column-major
            if idx >= total {
                continue;
            }
            let e = &panel.entries[idx];
            let mut name = e.name.clone();
            if e.is_dir() && e.name != ".." {
                name.push('/');
            }
            let text = truncate_width(&name, col_w.saturating_sub(1));
            let padded = format!("{:<width$}", text, width = col_w);
            let style = if active && idx == panel.cursor {
                Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg)
            } else if panel.marked.contains(&e.name) {
                Style::default().bg(theme.bg).fg(theme.mark_fg)
            } else {
                Style::default().bg(theme.bg).fg(theme.fg)
            };
            let cell = Rect::new(list_area.x + (sc * col_w) as u16, list_area.y + r as u16, col_w as u16, 1);
            f.render_widget(Paragraph::new(padded).style(style), cell);
        }
    }
    if let Some(info) = info_area {
        render_panel_footer(f, sep_area, info, panel, theme, active);
    }
}

/// Строка дерева: отступ по глубине + маркер ▸/▾ для директорий + имя (лист пути).
pub(super) fn tree_line(e: &VfsEntry, expanded: &std::collections::HashSet<String>, width: usize) -> String {
    let leaf = e.name.rsplit('/').next().unwrap_or(&e.name);
    let indent = "  ".repeat(e.depth);
    let marker = if e.name == ".." {
        "  "
    } else if e.is_dir() {
        if expanded.contains(&e.name) {
            "▾ "
        } else {
            "▸ "
        }
    } else {
        "  "
    };
    let mut s = format!("{indent}{marker}{leaf}");
    if e.is_dir() && e.name != ".." {
        s.push('/');
    }
    truncate_width(&s, width)
}

/// Рисует панель как сворачиваемое дерево (один столбец, отступы, маркеры).
pub(super) fn render_panel_tree(f: &mut Frame, area: Rect, panel: &mut Panel, theme: &Theme, active: bool) {
    let (block, inner) = panel_block(area, &panel.path.display().to_string(), theme, active);
    f.render_widget(block, area);
    let (list_area, sep_area, info_area) = split_info(inner);
    let inner_width = list_area.width as usize;
    let items: Vec<ListItem> = panel
        .entries
        .iter()
        .map(|e| {
            let item = ListItem::new(tree_line(e, &panel.expanded, inner_width));
            if panel.marked.contains(&e.name) {
                item.style(Style::default().fg(theme.mark_fg))
            } else {
                item
            }
        })
        .collect();
    if active {
        panel.state.select(Some(panel.cursor));
    } else {
        panel.state.select(None);
    }
    let hl = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);
    let list = List::new(items).highlight_style(hl);
    f.render_stateful_widget(list, list_area, &mut panel.state);
    if let Some(info) = info_area {
        render_panel_footer(f, sep_area, info, panel, theme, active);
    }
}

/// Рисует одну панель в заданной области.
pub(super) fn render_panel(f: &mut Frame, area: Rect, panel: &mut Panel, theme: &Theme, active: bool) {
    if panel.is_tree {
        render_panel_tree(f, area, panel, theme, active);
        return;
    }
    if panel.columns > 1 {
        render_panel_grid(f, area, panel, theme, active);
        return;
    }
    let (block, inner) = panel_block(area, &panel.path.display().to_string(), theme, active);
    f.render_widget(block, area);
    let (list_area, sep_area, info_area) = split_info(inner);

    let inner_width = list_area.width as usize;
    let items: Vec<ListItem> = panel
        .entries
        .iter()
        .map(|e| {
            let item = ListItem::new(format_entry(e, inner_width));
            if panel.marked.contains(&e.name) {
                item.style(Style::default().fg(theme.mark_fg))
            } else {
                item
            }
        })
        .collect();

    // Курсор показываем только на активной панели; на неактивной он скрыт.
    if active {
        panel.state.select(Some(panel.cursor));
    } else {
        panel.state.select(None);
    }
    let hl = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);
    let list = List::new(items).highlight_style(hl);
    f.render_stateful_widget(list, list_area, &mut panel.state);
    if let Some(info) = info_area {
        render_panel_footer(f, sep_area, info, panel, theme, active);
    }
}
