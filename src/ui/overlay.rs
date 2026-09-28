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
        SettingId::PanelView => (
            "file_list_view",
            match app.active_panel().view_override {
                None => "global",
                Some(FileListView::Flat) => "flat",
                Some(FileListView::Tree) => "tree",
            }
            .to_string(),
        ),
        SettingId::PanelColumns => ("columns", app.active_panel().columns.to_string()),
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
        SettingId::Save => ("", "[ Save ]".to_string()),
    }
}

/// Рисует экран настроек (`Ctrl+x x`): секции с заголовками (global/panel/viewer/editor).
pub(super) fn render_settings(f: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let st = app.settings.as_ref();
    let sel = st.map(|s| s.sel).unwrap_or(SETTINGS_FIRST);
    let editing = st.and_then(|s| s.editing.as_ref());

    let hint = "↑/↓ select · ←/→/Enter change · q/Esc close";
    let label_w = 22usize;
    let indent = 2usize; // отступ полей от края (заголовки — без отступа)
    let width = 52u16.min(area.width.saturating_sub(2));
    let height = (SETTINGS_ROWS.len() as u16 + 3).min(area.height); // строки + hint + рамка
    let rect = centered_rect(area, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Settings")
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);

    let iw = inner.width as usize;
    for (i, row) in SETTINGS_ROWS.iter().enumerate() {
        let rect_i = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        match row {
            SettingRow::Header(title) => {
                let text = format!("── {title} ──");
                let padded = format!("{:<width$}", truncate_width(&text, iw), width = iw);
                f.render_widget(
                    Paragraph::new(padded)
                        .style(Style::default().bg(theme.bg).fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
                    rect_i,
                );
            }
            SettingRow::Field(id) => {
                let ed = if i == sel { editing } else { None };
                let (label, value) = setting_label_value(app, *id, ed);
                let text = if label.is_empty() {
                    format!("{}{value}", " ".repeat(indent))
                } else {
                    format!("{}{label:<label_w$}{value}", " ".repeat(indent))
                };
                let style = if i == sel {
                    Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg)
                } else {
                    Style::default().bg(theme.bg).fg(theme.fg)
                };
                let padded = format!("{:<width$}", truncate_width(&text, iw), width = iw);
                f.render_widget(Paragraph::new(padded).style(style), rect_i);
            }
        }
    }
    let hy = inner.y + SETTINGS_ROWS.len() as u16;
    f.render_widget(
        Paragraph::new(truncate_width(hint, iw)).style(Style::default().bg(theme.bg).fg(Color::DarkGray)),
        Rect::new(inner.x, hy, inner.width, 1),
    );

    // Каретка при редактировании строкового значения (view/edit).
    if let Some(buf) = editing {
        let row_y = inner.y + sel as u16;
        let x = inner.x + (indent + label_w) as u16 + UnicodeWidthStr::width(buf.text().as_str()) as u16;
        f.set_cursor_position(Position::new(
            x.min(inner.x + inner.width.saturating_sub(1)),
            row_y,
        ));
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
        .title("History search (Ctrl+g)")
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

/// Рисует ряд кнопок по центру `area`; каждая подсвечена цветом курсора.
fn render_button_row(f: &mut Frame, area: Rect, buttons: &[&str], theme: &Theme) {
    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let hl = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, b) in buttons.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(BTN_GAP, base));
        }
        spans.push(Span::styled((*b).to_string(), hl));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).alignment(Alignment::Center), area);
}

/// Рисует модальный диалог (подтверждение или ввод) по центру экрана.
pub(super) fn render_dialog(f: &mut Frame, area: Rect, dialog: &Dialog, theme: &Theme) {
    // Подтверждение удаления — с кнопками Delete/Sudo/Cancel.
    if let Dialog::Confirm { message, .. } = dialog {
        render_confirm(f, area, message, theme);
        return;
    }
    let Dialog::Input { prompt, input, op } = dialog else {
        return; // Confirm — выше, HistorySearch — отдельно
    };
    let secret = is_secret(op);
    let body = if secret {
        "*".repeat(input.chars.len())
    } else {
        input.text()
    };
    // Кнопки только для copy/move (действия на Ctrl+Y/S/C).
    let buttons: Option<[&str; 3]> = match op {
        crate::app::PendingOp::Copy(_) => Some([" Copy (c-y) ", " Sudo (c-s) ", " Cancel (c-n) "]),
        crate::app::PendingOp::Move(_) => Some([" Move (c-y) ", " Sudo (c-s) ", " Cancel (c-n) "]),
        _ => None,
    };

    let btn_w = buttons.map(|b| button_row_width(&b)).unwrap_or(0);
    let content_w = UnicodeWidthStr::width(body.as_str())
        .max(UnicodeWidthStr::width(prompt.as_str()))
        .max(btn_w);
    let width = (content_w as u16 + 4).clamp(20, area.width.saturating_sub(2).max(20));
    // Рамка + строка ввода (+ пустая строка + кнопки, если есть).
    let height = if buttons.is_some() { 5 } else { 3 };
    let rect = centered_rect(area, width, height);

    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(prompt.clone())
        .style(base);
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);

    // Строка ввода.
    f.render_widget(
        Paragraph::new(body.clone()).style(base),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    // Ряд кнопок (для copy/move) — с пустой строкой-отступом над ними.
    if let Some(b) = buttons {
        render_button_row(f, Rect::new(inner.x, inner.y + 2, inner.width, 1), &b, theme);
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

/// Диалог ввода со скрытием значения (пароль).
fn is_secret(op: &crate::app::PendingOp) -> bool {
    matches!(op, crate::app::PendingOp::FtpConnect(_))
}

/// Рисует диалог подтверждения удаления: сообщение + строка кнопок
/// `Delete (Y)` / `Sudo (S)` / `Cancel (N)` (подсвечены как курсор).
fn render_confirm(f: &mut Frame, area: Rect, message: &str, theme: &Theme) {
    const BUTTONS: [&str; 3] = [" Delete (c-y) ", " Sudo (c-s) ", " Cancel (c-n) "];
    let inner_w = UnicodeWidthStr::width(message)
        .max(button_row_width(&BUTTONS))
        .max(UnicodeWidthStr::width("Confirm"));
    let width = (inner_w as u16 + 4).clamp(20, area.width.saturating_sub(2).max(20));
    let rect = centered_rect(area, width, 5); // рамка + сообщение + пустая строка + кнопки

    let base = Style::default().bg(theme.bg).fg(theme.fg);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Confirm")
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
    render_button_row(f, Rect::new(inner.x, inner.y + 2, inner.width, 1), &BUTTONS, theme);
}
