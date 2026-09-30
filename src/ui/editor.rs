//! Рендер встроенного редактора.

use super::*;
use crate::app::Editor;
use ratatui::text::{Line, Span};

/// Заменяет непечатные символы на `.` (ширина 1 — колонка соответствует символу).
fn sanitize_char(c: char) -> char {
    if c.is_control() {
        '.'
    } else {
        c
    }
}

/// Рисует встроенный редактор файла в области панели. Курсор — ячейка со стилем
/// курсора темы (терминальный курсор в TUI скрыт).
pub(super) fn render_editor(f: &mut Frame, area: Rect, ed: &mut Editor, theme: &Theme, active: bool) {
    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let border_style = if active {
        Style::default().bg(theme.bg).fg(theme.fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(theme.bg).fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!("Edit: {}{}", ed.name, if ed.dirty { " *" } else { "" }))
        .style(base);
    let inner = block.inner(area);
    f.render_widget(block, area);
    ed.page = inner.height as usize;
    ed.cols = inner.width as usize;
    // Курсор мог оказаться вне окна (первый рендер, resize) — подгоняем прокрутку.
    ed.ensure_visible(ed.page, ed.cols);

    let cur_style = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);
    // Выделенные строки (Shift+↑/↓) — фоном выделения темы.
    let sel = ed.sel_range();
    let sel_style = Style::default().bg(theme.select_bg).fg(theme.select_fg);
    for row in 0..inner.height {
        let li = ed.voff + row as usize;
        if li >= ed.lines.len() {
            break;
        }
        let base = if sel.is_some_and(|(a, b)| (a..=b).contains(&li)) { sel_style } else { base };
        let rect = Rect::new(inner.x, inner.y + row, inner.width, 1);
        let visible: String = ed.lines[li].chars().skip(ed.hoff).map(sanitize_char).collect();
        let line = if active && li == ed.cur_line {
            // Строка с курсором: до курсора / символ под курсором / после.
            let col = ed.cur_col - ed.hoff; // ensure_visible гарантирует cur_col >= hoff
            let chars: Vec<char> = visible.chars().collect();
            let pre: String = chars.iter().take(col).collect();
            let cur: String = chars.get(col).map_or(" ".to_string(), |c| c.to_string());
            let post: String = chars.iter().skip(col + 1).collect();
            Line::from(vec![
                Span::styled(pre, base),
                Span::styled(cur, cur_style),
                Span::styled(post, base),
            ])
        } else {
            Line::from(Span::styled(visible, base))
        };
        f.render_widget(Paragraph::new(line).style(base), rect);
    }
}
