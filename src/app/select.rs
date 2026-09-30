//! Пометка файлов по маске: `+` (select) / `-` (unselect) в пустой командной строке.

use super::*;

impl App {
    /// Открывает диалог `select`/`unselect`; в поле — последняя маска (или `*`).
    /// Маска живёт только в памяти (`last_mask`), в конфиг не пишется.
    pub(super) fn open_select_dialog(&mut self, select: bool) {
        let mask = self.last_mask.clone().unwrap_or_else(|| "*".to_string());
        self.dialog = Some(Dialog::Input {
            prompt: if select { "select" } else { "unselect" }.to_string(),
            input: CmdLine::from_str(&mask),
            op: PendingOp::Select(select),
        });
    }

    /// Помечает (или снимает пометку) записи активной панели, подходящие под glob-маску.
    /// Файлы и директории; `..` не затрагивается никогда. Регистрозависимо.
    pub(super) fn apply_mask(&mut self, mask: &str, select: bool) {
        let mask = mask.trim();
        if mask.is_empty() {
            self.status = "empty mask".to_string();
            return;
        }
        self.last_mask = Some(mask.to_string());
        let panel = self.active_panel_mut();
        let names: Vec<String> = panel
            .entries
            .iter()
            .filter(|e| e.name != "..")
            .filter(|e| crate::vfs::archive::glob_match(mask, &e.name))
            .map(|e| e.name.clone())
            .collect();
        for n in names {
            if select {
                panel.marked.insert(n);
            } else {
                panel.marked.remove(&n);
            }
        }
    }
}

/// Сводка пометок панели: (число помеченных записей, суммарный размер файлов).
/// Размер директорий не суммируется (ls-размер ничего не говорит, рекурсия дорогая).
pub fn marked_summary(panel: &Panel) -> (usize, u64) {
    panel
        .entries
        .iter()
        .filter(|e| panel.marked.contains(&e.name))
        .fold((0, 0), |(n, sz), e| {
            let add = if e.is_dir() { 0 } else { e.size.unwrap_or(0) };
            (n + 1, sz + add)
        })
}

#[cfg(test)]
#[path = "select_test.rs"]
mod tests;
