//! View/edit файлов не с локального слоя (SFTP/FTP, записи архивов): файл скачивается
//! во временную копию (`LocalCopy`), открывается во встроенном или внешнем
//! просмотрщике/редакторе; после правки копия заливается обратно. Временная
//! директория удаляется вместе с копией (при закрытии просмотрщика/редактора).

use super::*;
use crate::vfs::transfer::{Item, LocalCopy, Loc};

/// Внешний view/edit над временной копией: ждёт возврата из shell.
pub struct ExternalEdit {
    pub copy: LocalCopy,
    pub edit: bool,
    /// `(mtime, size)` копии до запуска — заливаем, только если редактор её изменил.
    pub before: Option<(std::time::SystemTime, u64)>,
}

impl App {
    /// Временная копия файла под курсором (не-локальный слой). `Ok(None)` — под курсором
    /// не файл. Запись архива — только для просмотра.
    pub(super) fn cursor_copy(&self, edit: bool) -> Result<Option<LocalCopy>, String> {
        let panel = self.active_panel();
        let Some(e) = panel.entries.get(panel.cursor) else {
            return Ok(None);
        };
        if e.name == ".." || e.is_dir() {
            return Ok(None);
        }
        let base = panel.path.cwd_loc()?;
        if edit && matches!(base, Loc::Archive(..)) {
            return Err("archive is read-only".to_string());
        }
        let item = Item {
            loc: base.join(&e.name),
            is_dir: false,
        };
        LocalCopy::fetch(item, if edit { "edit" } else { "view" })
            .map(Some)
            .map_err(|e| e.to_string())
    }

    /// Привязывает копию к только что открытому просмотрщику/редактору (заголовок —
    /// исходный путь). Если открыть не удалось — копия удаляется сразу.
    pub(super) fn attach_copy(&mut self, copy: LocalCopy, edit: bool) {
        let name = copy.origin.loc.display();
        let panel = self.active_panel_mut();
        if edit {
            if let Some(e) = panel.editor.as_mut() {
                e.name = name;
                e.copy = Some(copy);
            }
        } else if let Some(v) = panel.viewer.as_mut() {
            v.name = name;
            v.copy = Some(copy);
        }
    }

    /// Возврат из shell: если внешний редактор изменил временную копию — заливаем её
    /// обратно; затем перечитываем панели. При ошибке заливки копия сохраняется на диске.
    pub fn after_shell(&mut self) {
        if let Some(x) = self.pending_external.take() {
            if x.edit && x.copy.stamp() != x.before {
                let dest = x.copy.origin.loc.display();
                match x.copy.upload() {
                    Ok(()) => self.status = format!("uploaded {dest}"),
                    Err(e) => {
                        let kept = x.copy.keep();
                        self.status = format!("edit: upload failed: {e}; copy kept at {}", kept.display());
                    }
                }
            }
        }
        self.reload();
    }
}

#[cfg(test)]
#[path = "remote_file_test.rs"]
mod tests;
