//! Слой отображения (ratatui): панель со списком, строка статуса, командная строка.

use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{
    App, Completion, Dialog, Panel, SettingId, SettingRow, Viewer, SETTINGS_FIRST, SETTINGS_ROWS,
};
use crate::config::{FileListView, PanelLayout, PauseMode, SaveMode};
use crate::theme::Theme;
use crate::vfs::{EntryKind, VfsEntry};

mod editor;
mod overlay;
mod panel;
mod viewer;
use editor::render_editor;
use overlay::{render_dialog, render_history_search, render_settings};
use panel::render_panel;
use viewer::{render_help, render_viewer};

/// Обрезает строку по отображаемой ширине (unicode-width), не по байтам.
pub fn truncate_width(s: &str, max: usize) -> String {
    let mut width = 0usize;
    let mut out = String::new();
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if width + cw > max {
            break;
        }
        out.push(c);
        width += cw;
    }
    out
}

/// Рисует весь кадр.
pub fn render(f: &mut Frame, app: &mut App) {
    let theme = app.theme.clone();
    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Min(0),    // панели
        Constraint::Length(1), // статус/ошибки
        Constraint::Length(1), // командная строка
    ])
    .split(area);

    // ---- Панели (раскладка по panel_layout) ----
    let n = app.panels.len();
    let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Ratio(1, n as u32)).collect();
    let panel_areas = match app.config.panel_layout {
        // vertical = колонки бок о бок
        PanelLayout::Vertical => Layout::horizontal(constraints).split(chunks[0]),
        // horizontal = панели строками друг над другом
        PanelLayout::Horizontal => Layout::vertical(constraints).split(chunks[0]),
    };
    let active = app.active;
    for (i, panel) in app.panels.iter_mut().enumerate() {
        panel.area = panel_areas[i]; // запоминаем для попаданий мышью
        if let Some(ed) = panel.editor.as_mut() {
            render_editor(f, panel_areas[i], ed, &theme, i == active);
        } else if let Some(v) = panel.viewer.as_mut() {
            render_viewer(f, panel_areas[i], v, &theme, i == active);
        } else {
            render_panel(f, panel_areas[i], panel, &theme, i == active);
        }
    }

    // ---- Часы в правом верхнем углу на рамке (hh:mm), с отступом 1 символ от края ----
    if app.config.show_clock {
        let hm = crate::clock::hh_mm();
        let w = UnicodeWidthStr::width(hm.as_str()) as u16;
        if w > 0 && chunks[0].width > w + 2 {
            // Рамка нарисована стилем заголовка активной панели (жирным fg).
            let clock_style = Style::default().bg(theme.bg).fg(theme.fg).add_modifier(Modifier::BOLD);
            let x = chunks[0].x + chunks[0].width - 1 - w; // 1 символ от правого края
            f.render_widget(
                Paragraph::new(hm).style(clock_style),
                Rect::new(x, chunks[0].y, w, 1),
            );
        }
    }

    // ---- Строка статуса/подсказок/поиска — цветом командной строки ----
    let cmdline_style = Style::default().bg(theme.cmdline_bg).fg(theme.cmdline_fg);
    let status_width = chunks[1].width as usize;
    // Ввод строки поиска в просмотрщике активной панели (`/`).
    let viewer_find = app.panels[active].viewer.as_ref().and_then(|v| v.find_input.as_ref());
    if let Some(comp) = &app.completion {
        render_completion(f, chunks[1], comp, &theme);
    } else if let Some(query) = viewer_find {
        let line = format!("{:<width$}", format!("/{query}"), width = status_width);
        f.render_widget(Paragraph::new(line).style(cmdline_style), chunks[1]);
    } else if let Some(search) = &app.search {
        let line = format!("{:<width$}", format!("Search: {}", search.query), width = status_width);
        f.render_widget(Paragraph::new(line).style(cmdline_style), chunks[1]);
    } else {
        // Ошибки — красным, подсказки/сообщения — цветом текста командной строки.
        let is_error = !app.status.is_empty()
            && !app.status.starts_with("Ctrl+x")
            && !app.status.starts_with("viewer:")
            && !app.status.starts_with("config saved");
        let fg = if is_error { Color::Red } else { theme.cmdline_fg };
        let line = format!("{:<width$}", app.status, width = status_width);
        f.render_widget(
            Paragraph::new(line).style(Style::default().bg(theme.cmdline_bg).fg(fg)),
            chunks[1],
        );
    }

    // ---- Командная строка ----
    let prompt = app.prompt.as_str();
    let cmd_text = format!("{prompt}{}", app.cmdline.text());
    // Заполняем всю строку фоном командной строки.
    let padded_cmd = format!("{:<width$}", cmd_text, width = chunks[2].width as usize);
    let cmd = Paragraph::new(padded_cmd).style(cmdline_style);
    f.render_widget(cmd, chunks[2]);

    // ---- Окно помощи поверх всего ----
    if let Some(help) = app.help.as_mut() {
        render_help(f, area, help, &theme);
        return;
    }

    // ---- Экран настроек ----
    if app.settings.is_some() {
        render_settings(f, area, app, &theme);
        return;
    }

    // ---- Модальный диалог поверх всего ----
    if let Some(dialog) = &app.dialog {
        match dialog {
            Dialog::HistorySearch {
                input,
                results,
                sel,
            } => render_history_search(f, area, input, results, *sel, &theme),
            _ => render_dialog(f, area, dialog, &theme),
        }
        return;
    }

    // Каретка: в строке поиска просмотрщика (`/`), в строке поиска по списку, либо в командной строке.
    if let Some(query) = viewer_find {
        let qx = 1 + UnicodeWidthStr::width(query.as_str()) as u16; // после '/'
        let x = chunks[1]
            .x
            .saturating_add(qx)
            .min(chunks[1].x + chunks[1].width.saturating_sub(1));
        f.set_cursor_position(Position::new(x, chunks[1].y));
    } else if let Some(search) = &app.search {
        let prefix_w = UnicodeWidthStr::width("Search: ") as u16;
        let qx = prefix_w + UnicodeWidthStr::width(search.query.as_str()) as u16;
        let x = chunks[1]
            .x
            .saturating_add(qx)
            .min(chunks[1].x + chunks[1].width.saturating_sub(1));
        f.set_cursor_position(Position::new(x, chunks[1].y));
    } else {
        let before: String = app.cmdline.chars[..app.cmdline.cursor].iter().collect();
        let caret =
            UnicodeWidthStr::width(app.prompt.as_str()) as u16 + UnicodeWidthStr::width(before.as_str()) as u16;
        let x = chunks[2]
            .x
            .saturating_add(caret)
            .min(chunks[2].x + chunks[2].width.saturating_sub(1));
        f.set_cursor_position(Position::new(x, chunks[2].y));
    }
}

/// Рисует варианты автодополнения в строке хинтов; выбранный — цветом курсора.
/// Горизонтально сдвигается так, чтобы выбранный вариант был виден.
fn render_completion(f: &mut Frame, area: Rect, comp: &Completion, theme: &Theme) {
    let width = area.width as usize;
    if width == 0 {
        return;
    }
    let base = Style::default().bg(theme.cmdline_bg).fg(theme.cmdline_fg);
    let hl = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);

    // Последовательность (символ, выбран?) с разделителями между вариантами.
    let mut seq: Vec<(char, bool)> = Vec::new();
    let mut sel_start = 0usize;
    let mut sel_len = 0usize;
    for (i, cand) in comp.candidates.iter().enumerate() {
        if i > 0 {
            seq.push((' ', false));
            seq.push((' ', false));
        }
        if i == comp.sel {
            sel_start = seq.len();
            sel_len = cand.chars().count();
        }
        for ch in cand.chars() {
            seq.push((ch, i == comp.sel));
        }
    }
    // Сдвиг, чтобы конец выбранного помещался в ширину.
    let sel_end = sel_start + sel_len;
    let off = sel_end.saturating_sub(width);
    let visible: Vec<(char, bool)> = seq.iter().skip(off).take(width).copied().collect();

    // Группируем подряд идущие символы одного стиля в спаны.
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut cur = false;
    for (ch, s) in &visible {
        if *s != cur && !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut buf), if cur { hl } else { base }));
        }
        cur = *s;
        buf.push(*ch);
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, if cur { hl } else { base }));
    }
    if visible.len() < width {
        spans.push(Span::styled(" ".repeat(width - visible.len()), base));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Центрированный прямоугольник заданной ширины/высоты в области `area`.
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

#[cfg(test)]
#[path = "ui_test.rs"]
mod tests;
