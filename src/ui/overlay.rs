//! Рендер модальных окон: настройки, поиск по истории, диалоги.

use super::*;
use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};

/// Метка и текущее значение параметра для показа в окне настроек.
/// `editing` — буфер редактируемой строки (view/edit), если выбрана именно эта строка.
pub(super) fn setting_label_value(app: &App, id: SettingId, editing: Option<&crate::app::CmdLine>) -> (&'static str, String) {
    let c = &app.config;
    match id {
        SettingId::ShowHidden => ("show_hidden", c.show_hidden.to_string()),
        SettingId::ShowClock => ("show_clock", c.show_clock.to_string()),
        SettingId::PanelLayout => (
            "panel_layout",
            match c.panel_layout {
                PanelLayout::Vertical => "vertical",
                PanelLayout::Horizontal => "horizontal",
            }
            .to_string(),
        ),
        SettingId::FileListView => (
            "file_list_view",
            match c.file_list_view {
                FileListView::Flat => "flat",
                FileListView::Tree => "tree",
            }
            .to_string(),
        ),
        SettingId::Theme => ("theme", c.theme.clone()),
        SettingId::PauseAfterCommand => (
            "pause_after_command",
            match c.pause_after_command {
                PauseMode::Always => "always",
                PauseMode::OnOutput => "on_output",
                PauseMode::Never => "never",
            }
            .to_string(),
        ),
        SettingId::ConfigSave => (
            "config_save",
            match c.config_save {
                SaveMode::Always => "always",
                SaveMode::OnChange => "on-change",
                SaveMode::OnSave => "on-save",
            }
            .to_string(),
        ),
        SettingId::PanelView(i) => (
            "file_list_view",
            match app.panels.get(i).and_then(|p| p.view_override) {
                None => "global",
                Some(FileListView::Flat) => "flat",
                Some(FileListView::Tree) => "tree",
            }
            .to_string(),
        ),
        SettingId::PanelColumns(i) => (
            "columns",
            app.panels.get(i).map(|p| p.columns).unwrap_or(1).to_string(),
        ),
        SettingId::View => (
            "view",
            editing.map(|b| b.text()).unwrap_or_else(|| c.view.clone()),
        ),
        SettingId::ViewHex => ("hex", c.view_hex.to_string()),
        SettingId::ViewWrap => ("wrap", c.view_wrap.to_string()),
        SettingId::Edit => (
            "edit",
            editing.map(|b| b.text()).unwrap_or_else(|| c.edit.clone()),
        ),
    }
}

/// Раскладка окна настроек: позиция каждой строки `(колонка, y)` относительно
/// внутренней области и число строк содержимого. Две колонки: слева Global/Viewer/Editor,
/// справа — секции панелей, строка кнопок — под обеими (через пустую строку). Одна колонка —
/// все строки подряд.
pub(super) fn settings_positions(n_rows: usize, two_cols: bool) -> (Vec<(usize, u16)>, u16) {
    let left = SETTINGS_LEFT.len();
    let save = n_rows - 1;
    if !two_cols {
        return ((0..n_rows).map(|i| (0, i as u16)).collect(), n_rows as u16);
    }
    let right = save - left;
    let body = left.max(right) as u16;
    let mut pos = Vec::with_capacity(n_rows);
    pos.extend((0..left).map(|i| (0, i as u16)));
    pos.extend((0..right).map(|j| (1, j as u16)));
    pos.push((0, body + 1));
    (pos, body + 2)
}

/// Хвост строки, влезающий в `width` (для путей: важнее конец), с `…` в начале.
fn tail_width(s: &str, width: usize) -> String {
    if UnicodeWidthStr::width(s) <= width {
        return s.to_string();
    }
    let mut out: Vec<char> = Vec::new();
    let mut w = 1; // «…»
    for c in s.chars().rev() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > width {
            break;
        }
        w += cw;
        out.push(c);
    }
    out.reverse();
    format!("…{}", out.into_iter().collect::<String>())
}

/// Рисует экран настроек (`Ctrl+x x`): слева Global/Viewer/Editor, справа — по секции
/// на каждую панель; на узком экране — одна колонка.
pub(super) fn render_settings(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    const COL_MIN: u16 = 34;
    const COL_MAX: u16 = 44;
    let st = app.settings.as_ref();
    let sel = st.map(|s| s.sel).unwrap_or(SETTINGS_FIRST);
    let editing = st.and_then(|s| s.editing.as_ref());
    let rows = app.settings_rows();

    let hint = "↑/↓ select · Tab section · ←/→/Enter change";
    let label_w = 20usize;
    let indent = 2usize; // отступ полей от края колонки (заголовки — без отступа)
    // Две колонки, если каждая получает хотя бы COL_MIN (рамка 2 + зазор 1).
    let half = area.width.saturating_sub(3) / 2;
    let two_cols = half >= COL_MIN;
    let col_w = if two_cols { half.min(COL_MAX) } else { 52u16.min(area.width.saturating_sub(2)) };
    let width = if two_cols { col_w * 2 + 3 } else { col_w + 2 };
    let (pos, body_h) = settings_positions(rows.len(), two_cols);
    let height = (body_h + 3).min(area.height); // содержимое + hint + рамка
    let rect = centered_rect(area, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("[ Settings ]")
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);

    let header_style = Style::default().bg(theme.bg).fg(Color::DarkGray).add_modifier(Modifier::BOLD);
    let mut caret: Option<Position> = None;
    for (i, row) in rows.iter().enumerate() {
        let (col, y) = pos[i];
        if y >= inner.height.saturating_sub(1) {
            continue; // не влезает (последняя строка — hint)
        }
        let x = inner.x + col as u16 * (col_w + 1);
        // Строка кнопок — по центру на всю ширину окна.
        if let SettingRow::Buttons = row {
            let focus = (i == sel).then(|| st.map(|s| s.button).unwrap_or(0));
            render_button_row(f, Rect::new(inner.x, inner.y + y, inner.width, 1), &SETTINGS_BUTTONS, focus, theme);
            continue;
        }
        let rect_i = Rect::new(x, inner.y + y, col_w.min(inner.x + inner.width - x), 1);
        let w = rect_i.width as usize;
        let (text, style) = match row {
            SettingRow::Header(title) => (format!("── {title} ──"), header_style),
            SettingRow::PanelHeader(p) => {
                let active = if *p == app.active { " (active)" } else { "" };
                (format!("── Panel {}{active} ──", p + 1), header_style)
            }
            SettingRow::PanelDir(p) => {
                let dir = app.panels.get(*p).map(|p| p.path.display()).unwrap_or_default();
                (
                    format!("{}{}", " ".repeat(indent), tail_width(&dir, w.saturating_sub(indent))),
                    Style::default().bg(theme.bg).fg(Color::DarkGray),
                )
            }
            SettingRow::Buttons => continue, // нарисована выше
            SettingRow::Field(id) => {
                let ed = if i == sel { editing } else { None };
                let (label, value) = setting_label_value(app, *id, ed);
                let text = format!("{}{label:<label_w$}{value}", " ".repeat(indent));
                let style = if i == sel {
                    Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg)
                } else {
                    Style::default().bg(theme.bg).fg(theme.fg)
                };
                if let (true, Some(buf)) = (i == sel, ed) {
                    let cx = x + (indent + label_w) as u16 + UnicodeWidthStr::width(buf.text().as_str()) as u16;
                    caret = Some(Position::new(cx.min(x + col_w.saturating_sub(1)), inner.y + y));
                }
                (text, style)
            }
        };
        let padded = format!("{:<width$}", truncate_width(&text, w), width = w);
        f.render_widget(Paragraph::new(padded).style(style), rect_i);
    }
    let hy = inner.y + inner.height.saturating_sub(1);
    f.render_widget(
        Paragraph::new(truncate_width(hint, inner.width as usize))
            .style(Style::default().bg(theme.bg).fg(Color::DarkGray)),
        Rect::new(inner.x, hy, inner.width, 1),
    );

    // Каретка при редактировании строкового значения (view/edit).
    if let Some(p) = caret {
        f.set_cursor_position(p);
    }
}

/// Рисует диалог поиска по истории: строка запроса + список результатов.
pub(super) fn render_history_search(
    f: &mut Frame,
    area: Rect,
    input: &crate::app::CmdLine,
    results: &[String],
    sel: usize,
    theme: &Theme,
) {
    let max_rows = 12usize;
    let shown = results.len().min(max_rows);
    let width = area.width.saturating_sub(4).clamp(20, 80);
    let height = (shown as u16 + 3).min(area.height); // рамка + строка запроса
    let rect = centered_rect(area, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("[ History search (Ctrl+g) ]")
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);

    let inner_w = rect.width.saturating_sub(2) as usize;
    // Строка запроса.
    let query = format!("> {}", input.text());
    let qpar = Paragraph::new(truncate_width(&query, inner_w))
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let qarea = Rect::new(rect.x + 1, rect.y + 1, rect.width.saturating_sub(2), 1);
    f.render_widget(qpar, qarea);

    // Результаты.
    for (i, cmd) in results.iter().take(shown).enumerate() {
        let y = rect.y + 2 + i as u16;
        let row = Rect::new(rect.x + 1, y, rect.width.saturating_sub(2), 1);
        let text = truncate_width(cmd, inner_w);
        let style = if i == sel {
            Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg)
        } else {
            Style::default().bg(theme.bg).fg(theme.fg)
        };
        // Заполняем строку до ширины, чтобы подсветка занимала всю строку.
        let padded = format!("{:<width$}", text, width = inner_w);
        f.render_widget(Paragraph::new(padded).style(style), row);
    }

    // Каретка в строке запроса.
    let before: String = input.chars[..input.cursor].iter().collect();
    let x = rect.x + 3 + UnicodeWidthStr::width(before.as_str()) as u16;
    f.set_cursor_position(Position::new(
        x.min(rect.x + rect.width.saturating_sub(2)),
        rect.y + 1,
    ));
}

/// Разделитель между кнопками.
const BTN_GAP: &str = "  ";

/// Отображаемая ширина ряда кнопок (с разделителями).
fn button_row_width(buttons: &[&str]) -> usize {
    buttons.iter().map(|b| UnicodeWidthStr::width(*b)).sum::<usize>()
        + BTN_GAP.len() * buttons.len().saturating_sub(1)
}

/// Кнопки окна настроек.
const SETTINGS_BUTTONS: [&str; 2] = ["[ Save (c-s) ]", "[ Cancel (c-n) ]"];

/// Рисует ряд кнопок по центру `area`; каждая подсвечена цветом курсора.
/// `focus` — кнопка в фокусе: цвета `button_sel_bg`/`button_sel_fg` темы.
fn render_button_row(f: &mut Frame, area: Rect, buttons: &[&str], focus: Option<usize>, theme: &Theme) {
    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let hl = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, b) in buttons.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(BTN_GAP, base));
        }
        let sel = Style::default().bg(theme.button_sel_bg).fg(theme.button_sel_fg);
        let style = if focus == Some(i) { sel } else { hl };
        spans.push(Span::styled((*b).to_string(), style));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).alignment(Alignment::Center), area);
}

/// Рисует модальный диалог (подтверждение или ввод) по центру экрана.
/// `focus` — индекс кнопки в фокусе (`App.dialog_btn`).
pub(super) fn render_dialog(f: &mut Frame, area: Rect, dialog: &Dialog, focus: usize, theme: &Theme) {
    let labels: Vec<&str> = crate::app::dialog_buttons(dialog).iter().map(|(l, _)| *l).collect();
    let focus = Some(focus.min(labels.len().saturating_sub(1)));
    // Подтверждение (удаление / выход из редактора без сохранения) — с кнопками.
    if let Dialog::Confirm { message, .. } = dialog {
        render_confirm(f, area, message, &labels, focus, theme);
        return;
    }
    if let Dialog::Replace { find, repl, field } = dialog {
        render_replace(f, area, [find, repl], *field, &labels, focus, theme);
        return;
    }
    let Dialog::Input { prompt, input, op } = dialog else {
        return; // Confirm/Replace — выше, HistorySearch — отдельно
    };
    let secret = is_secret(op);
    let body = if secret {
        "*".repeat(input.chars.len())
    } else {
        input.text()
    };
    // Кнопки для copy/move/select/create (действия на Ctrl+Y/S/N).
    let buttons: Option<&[&str]> = (!labels.is_empty()).then_some(labels.as_slice());

    let title = input_dialog_title(prompt);
    let btn_w = buttons.map(button_row_width).unwrap_or(0);
    let content_w = UnicodeWidthStr::width(body.as_str())
        .max(UnicodeWidthStr::width(title.as_str()))
        .max(btn_w);
    let width = input_dialog_width(content_w, width_pct(op), area.width);
    // Рамка + строка ввода (+ пустая строка + кнопки, если есть).
    let height = if buttons.is_some() { 5 } else { 3 };
    let rect = centered_rect(area, width, height);

    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(base);
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);

    // Строка ввода.
    f.render_widget(
        Paragraph::new(body.clone()).style(base),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    // Ряд кнопок (для copy/move/select/create) — с пустой строкой-отступом над ними.
    if let Some(b) = buttons {
        render_button_row(f, Rect::new(inner.x, inner.y + 2, inner.width, 1), b, focus, theme);
    }

    // Каретка в поле ввода.
    let caret = if secret {
        input.cursor as u16 // звёздочки шириной 1
    } else {
        let before: String = input.chars[..input.cursor].iter().collect();
        UnicodeWidthStr::width(before.as_str()) as u16
    };
    let x = inner.x + caret;
    f.set_cursor_position(Position::new(
        x.min(inner.x + inner.width.saturating_sub(1)),
        inner.y,
    ));
}

/// Подписи полей диалога замены (выровнены по ширине).
const REPLACE_LABELS: [&str; 2] = ["Find:    ", "Replace: "];

/// Рисует диалог замены в редакторе (`Ctrl+r`): поля поиска и замены,
/// пустая строка и ряд кнопок; каретка — в активном поле `field`.
fn render_replace(
    f: &mut Frame,
    area: Rect,
    inputs: [&crate::app::CmdLine; 2],
    field: usize,
    buttons: &[&str],
    focus: Option<usize>,
    theme: &Theme,
) {
    let title = input_dialog_title("replace");
    let label_w = UnicodeWidthStr::width(REPLACE_LABELS[0]);
    let content_w = inputs
        .iter()
        .map(|i| label_w + UnicodeWidthStr::width(i.text().as_str()))
        .max()
        .unwrap_or(0)
        .max(UnicodeWidthStr::width(title.as_str()))
        .max(button_row_width(buttons));
    let width = input_dialog_width(content_w, 115, area.width);
    // Рамка + два поля + пустая строка + кнопки.
    let rect = centered_rect(area, width, 6);

    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let block = Block::default().borders(Borders::ALL).title(title).style(base);
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);

    for (row, (label, input)) in REPLACE_LABELS.iter().zip(inputs).enumerate() {
        f.render_widget(
            Paragraph::new(format!("{label}{}", input.text())).style(base),
            Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
        );
    }
    render_button_row(f, Rect::new(inner.x, inner.y + 3, inner.width, 1), buttons, focus, theme);

    let field = field.min(1);
    let input = inputs[field];
    let before: String = input.chars[..input.cursor].iter().collect();
    let x = inner.x + (label_w + UnicodeWidthStr::width(before.as_str())) as u16;
    f.set_cursor_position(Position::new(
        x.min(inner.x + inner.width.saturating_sub(1)),
        inner.y + field as u16,
    ));
}

/// Диалоги create/copy/move, select/unselect и поиска в редакторе — шире содержимого.
fn is_file_op(op: &crate::app::PendingOp) -> bool {
    use crate::app::PendingOp::*;
    matches!(op, MkDir(_) | Copy(_) | Move(_) | Select(_) | EditorSearch)
}

/// Заголовок диалога ввода — в скобках, как у активной панели.
pub(super) fn input_dialog_title(prompt: &str) -> String {
    format!("[ {prompt} ]")
}

/// Ширина содержимого диалога ввода в процентах: create — +50%,
/// copy/move/select — +15%, прочие — как есть.
fn width_pct(op: &crate::app::PendingOp) -> usize {
    match op {
        crate::app::PendingOp::MkDir(_) => 150,
        op if is_file_op(op) => 115,
        _ => 100,
    }
}

/// Ширина диалога ввода: содержимое × `pct`% + 4 на рамку и поля.
/// Не меньше 20, не шире экрана.
pub(super) fn input_dialog_width(content_w: usize, pct: usize, screen_w: u16) -> u16 {
    let w = content_w * pct / 100;
    (w.min(u16::MAX as usize) as u16)
        .saturating_add(4)
        .clamp(20, screen_w.saturating_sub(2).max(20))
}

/// Диалог ввода со скрытием значения (пароль).
fn is_secret(op: &crate::app::PendingOp) -> bool {
    matches!(op, crate::app::PendingOp::FtpConnect(_))
}

/// Рисует диалог подтверждения: сообщение + строка кнопок (подсвечены как курсор,
/// кнопка в фокусе — `button_sel_bg`).
fn render_confirm(f: &mut Frame, area: Rect, message: &str, buttons: &[&str], focus: Option<usize>, theme: &Theme) {
    let inner_w = UnicodeWidthStr::width(message)
        .max(button_row_width(buttons))
        .max(UnicodeWidthStr::width("[ Confirm ]"));
    let width = (inner_w as u16 + 4).clamp(20, area.width.saturating_sub(2).max(20));
    let rect = centered_rect(area, width, 5); // рамка + сообщение + пустая строка + кнопки

    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("[ Confirm ]")
        .style(base);
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);

    // Строка сообщения (по центру).
    let msg = truncate_width(message, inner.width as usize);
    f.render_widget(
        Paragraph::new(msg).style(base).alignment(Alignment::Center),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    // Строка кнопок (с пустой строкой-отступом над ними).
    render_button_row(f, Rect::new(inner.x, inner.y + 2, inner.width, 1), buttons, focus, theme);
}
