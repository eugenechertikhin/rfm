//! Рендер встроенного просмотрщика и окна помощи.

use super::*;
use ratatui::text::{Line, Span};

/// Рисует встроенный просмотрщик файла в области панели.
pub(super) fn render_viewer(f: &mut Frame, area: Rect, v: &mut Viewer, theme: &Theme, active: bool) {
    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let border_style = if active {
        Style::default().bg(theme.bg).fg(theme.fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(theme.bg).fg(Color::DarkGray)
    };
    let mode = if v.hex { " [hex]" } else if v.wrap { " [wrap]" } else { "" };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!("View: {}{}", v.name, mode))
        .style(base);
    let inner = block.inner(area);
    f.render_widget(block, area);
    v.page = inner.height as usize; // для Space/PageDown
    v.cols = inner.width as usize; // для гориз. прыжка к совпадению
    let width = inner.width as usize;
    let hoff = v.hoff;
    let wrap = v.wrap && !v.hex;
    let voff = v.voff;

    // Стили подсветки поиска: совпадение — жирным, текущее — как курсор.
    let match_style = base.add_modifier(Modifier::BOLD);
    let cur_style = Style::default()
        .bg(theme.cursor_bg)
        .fg(theme.cursor_fg)
        .add_modifier(Modifier::BOLD);

    // Тянем из файла только видимое окно (каждая исходная строка даёт ≥1 экранной).
    let lines = v.window(voff, inner.height as usize);
    let find = v.find.as_ref();

    let mut row = 0u16;
    'outer: for (off, line) in lines.iter().enumerate() {
        if row >= inner.height {
            break;
        }
        let li = voff + off;
        // Совпадения в этой строке и колонка текущего (если оно здесь).
        let (cols_here, current_col, qlen) = match find {
            Some(f) => {
                let here: Vec<usize> =
                    f.matches.iter().filter(|(l, _)| *l == li).map(|(_, c)| *c).collect();
                let cur = f
                    .matches
                    .get(f.current)
                    .filter(|(l, _)| *l == li)
                    .map(|(_, c)| *c);
                (here, cur, f.qlen)
            }
            None => (Vec::new(), None, 0),
        };
        // Сегменты: при wrap — по ширине с накоплением смещения; иначе один срез от hoff.
        let segments: Vec<(usize, String)> = if wrap {
            let mut acc = 0usize;
            wrap_line(line, width.max(1))
                .into_iter()
                .map(|p| {
                    let start = acc;
                    acc += p.chars().count();
                    (start, p)
                })
                .collect()
        } else {
            vec![(hoff, line.chars().skip(hoff).collect::<String>())]
        };
        for (start_col, seg) in segments {
            if row >= inner.height {
                break 'outer;
            }
            let rect = Rect::new(inner.x, inner.y + row, inner.width, 1);
            let styled = highlight_segment(
                &seg, start_col, &cols_here, current_col, qlen, width, base, match_style, cur_style,
            );
            f.render_widget(Paragraph::new(styled).style(base), rect);
            row += 1;
        }
    }
}

/// Класс подсветки символа по абсолютной колонке.
#[derive(PartialEq, Clone, Copy)]
enum Hl {
    Base,
    Match,
    Current,
}

fn hl_at(abs: usize, cols: &[usize], current: Option<usize>, qlen: usize) -> Hl {
    if qlen == 0 {
        return Hl::Base;
    }
    if let Some(c) = current {
        if abs >= c && abs < c + qlen {
            return Hl::Current;
        }
    }
    if cols.iter().any(|&c| abs >= c && abs < c + qlen) {
        Hl::Match
    } else {
        Hl::Base
    }
}

/// Собирает экранный сегмент в `Line` со стилями: совпадения — `match_style`,
/// текущее — `cur_style`, остальное — `base`. `start_col` — символьная колонка
/// начала сегмента в исходной строке; обрезка по `max_width` (unicode-width).
#[allow(clippy::too_many_arguments)]
fn highlight_segment(
    seg: &str,
    start_col: usize,
    cols: &[usize],
    current: Option<usize>,
    qlen: usize,
    max_width: usize,
    base: Style,
    match_style: Style,
    cur_style: Style,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut buf_hl = Hl::Base;
    let mut width = 0usize;
    for (i, ch) in seg.chars().enumerate() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw > max_width {
            break;
        }
        let hl = hl_at(start_col + i, cols, current, qlen);
        if hl != buf_hl && !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut buf), style_of(buf_hl, base, match_style, cur_style)));
        }
        buf_hl = hl;
        buf.push(ch);
        width += cw;
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, style_of(buf_hl, base, match_style, cur_style)));
    }
    Line::from(spans)
}

fn style_of(hl: Hl, base: Style, match_style: Style, cur_style: Style) -> Style {
    match hl {
        Hl::Base => base,
        Hl::Match => match_style,
        Hl::Current => cur_style,
    }
}

/// Разбивает строку на сегменты по ширине терминала (по unicode-width).
pub(super) fn wrap_line(s: &str, width: usize) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            w = 0;
        }
        cur.push(c);
        w += cw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Рисует окно помощи как просмотрщик со скроллом; размер — по содержимому (как раньше).
pub(super) fn render_help(f: &mut Frame, area: Rect, v: &mut Viewer, theme: &Theme) {
    // Help — небольшой in-memory просмотрщик; берём его строки напрямую.
    let mem: Vec<String> = v.content_lines().cloned().unwrap_or_default();
    let content_w = mem
        .iter()
        .map(|l| UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(20) as u16;
    let width = (content_w + 4).min(area.width);
    let height = (mem.len() as u16 + 2).min(area.height);
    let rect = centered_rect(area, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Help")
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    v.page = inner.height as usize;

    let base = Style::default().bg(theme.bg).fg(theme.fg);
    for row in 0..inner.height {
        let li = v.voff + row as usize;
        if li >= mem.len() {
            break;
        }
        let shown: String = mem[li].chars().skip(v.hoff).collect();
        let text = truncate_width(&shown, inner.width as usize);
        f.render_widget(
            Paragraph::new(text).style(base),
            Rect::new(inner.x, inner.y + row, inner.width, 1),
        );
    }
}
