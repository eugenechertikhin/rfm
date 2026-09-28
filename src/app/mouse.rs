//! Обработка мыши: клики по панелям (выбор панели/курсор/двойной клик) и колесо.

use super::*;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

impl App {
    /// Обработка мыши: клик — выбрать панель и поставить курсор на файл (двойной —
    /// действие как по `Enter`); колесо — двигать курсор активной панели.
    pub fn handle_mouse(&mut self, me: MouseEvent) -> Action {
        // Колесо в скроллируемых окнах (настройки/help/просмотрщик) — листает их;
        // клики в них игнорируем (клик обрабатывают только панели).
        let wheel = match me.kind {
            MouseEventKind::ScrollUp => Some(-1isize),
            MouseEventKind::ScrollDown => Some(1isize),
            _ => None,
        };
        // Диалог и инкрементальный поиск мышь не обрабатывают вовсе.
        if self.dialog.is_some() || self.search.is_some() {
            return Action::None;
        }
        if self.settings.is_some() {
            return match wheel {
                Some(d) => {
                    self.settings_move(d);
                    Action::Redraw
                }
                None => Action::None,
            };
        }
        if let Some(help) = self.help.as_mut() {
            return match wheel {
                Some(d) => {
                    help.scroll(d);
                    Action::Redraw
                }
                None => Action::None,
            };
        }
        // Редактор/просмотрщик в активной панели: колесо листает содержимое.
        if let Some(d) = wheel {
            if let Some(ed) = self.active_panel_mut().editor.as_mut() {
                ed.scroll(d);
                let (p, c) = (ed.page, ed.cols);
                ed.ensure_visible(p, c);
                return Action::Redraw;
            }
            if let Some(v) = self.active_panel_mut().viewer.as_mut() {
                v.scroll(d);
                return Action::Redraw;
            }
        }
        let action = match me.kind {
            MouseEventKind::ScrollUp => {
                self.move_cursor(-1);
                Action::None
            }
            MouseEventKind::ScrollDown => {
                self.move_cursor(1);
                Action::None
            }
            MouseEventKind::Down(MouseButton::Left) => self.handle_click(me.column, me.row),
            _ => Action::None,
        };
        if self.panels_dirty {
            self.save_panels();
            self.panels_dirty = false;
        }
        action
    }

    /// Клик левой кнопкой: активировать панель под указателем и, если попали в строку
    /// файла, поставить туда курсор. Второй клик по той же строке в пределах 400 мс —
    /// действие как по `Enter` (открыть каталог / запустить `+x` / войти в архив).
    fn handle_click(&mut self, col: u16, row: u16) -> Action {
        let Some(pi) = self.panels.iter().position(|p| {
            let a = p.area;
            col >= a.x && col < a.x + a.width && row >= a.y && row < a.y + a.height
        }) else {
            return Action::None;
        };
        self.active = pi;
        self.panels_dirty = true;
        // Панель с редактором: клик по тексту ставит курсор редактора
        // (учитываются рамка, прокрутка и ширина символов); двойной клик роли не играет.
        if self.panels[pi].editor.is_some() {
            let a = self.panels[pi].area;
            if a.width >= 3
                && a.height >= 3
                && col >= a.x + 1
                && col < a.x + a.width - 1
                && row >= a.y + 1
                && row < a.y + a.height - 1
            {
                let rel_row = (row - a.y - 1) as usize;
                let rel_col = (col - a.x - 1) as usize;
                if let Some(ed) = self.panels[pi].editor.as_mut() {
                    ed.click_at(rel_row, rel_col);
                }
            }
            self.last_click = None;
            return Action::Redraw;
        }
        let Some(idx) = self.click_entry_index(pi, col, row) else {
            // Клик по рамке/заголовку/пустому месту — только выбрали панель.
            self.last_click = None;
            return Action::Redraw;
        };
        self.panels[pi].cursor = idx;
        let now = std::time::Instant::now();
        let is_double = matches!(
            self.last_click,
            Some((t, p, i))
                if p == pi && i == idx
                    && now.duration_since(t) < std::time::Duration::from_millis(400)
        );
        if is_double {
            self.last_click = None;
            return self.activate_cursor();
        }
        self.last_click = Some((now, pi, idx));
        Action::Redraw
    }

    /// Индекс записи под точкой `(col, row)` в панели `pi`, если точка попадает в строку
    /// файла (учитывает рамку, прокрутку списка и многоколоночную раскладку).
    fn click_entry_index(&self, pi: usize, col: u16, row: u16) -> Option<usize> {
        let p = &self.panels[pi];
        if p.viewer.is_some() || p.editor.is_some() {
            return None; // в панели открыт просмотрщик/редактор — только активируем её
        }
        let a = p.area;
        if a.width < 3 || a.height < 3 {
            return None;
        }
        // Внутренняя область — внутри рамки (1px со всех сторон).
        let (ix, iy) = (a.x + 1, a.y + 1);
        let (iw, ih) = (a.width - 2, a.height - 2);
        // Нижние строки внутри рамки заняты разделителем и инфо-строкой (когда есть
        // место: 2 строки при высоте >= 3, 1 при высоте 2) — они не кликабельны.
        let lh = if ih >= 3 { ih - 2 } else if ih == 2 { ih - 1 } else { ih };
        if col < ix || col >= ix + iw || row < iy || row >= iy + lh {
            return None;
        }
        let rel_row = (row - iy) as usize;
        let rel_col = (col - ix) as usize;
        let idx = if p.columns > 1 {
            let cols = p.columns.max(1);
            let rows = p.grid_rows.max(1);
            let col_w = (iw as usize / cols).max(1);
            let which_col = p.grid_left + rel_col / col_w;
            which_col * rows + rel_row
        } else {
            // Плоский список / дерево (ratatui List): учитываем прокрутку offset().
            p.state.offset() + rel_row
        };
        (idx < p.entries.len()).then_some(idx)
    }
}
