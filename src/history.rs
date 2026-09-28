//! История введённых команд: хранится в `$XDG_DATA_HOME/rfm/history`
//! (дефолт `~/.local/share/rfm/history`). Для каждой команды — счётчик частоты
//! и время последнего использования (unix-секунды).
//!
//! Формат: по строке на команду, TSV `count\tlast\tcmd`.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistEntry {
    pub cmd: String,
    pub count: u32,
    pub last: u64,
}

#[derive(Debug, Default)]
pub struct History {
    pub entries: Vec<HistEntry>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Путь к файлу истории (по XDG data-dir).
fn data_file() -> Option<PathBuf> {
    if let Ok(x) = env::var("XDG_DATA_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("rfm/history"));
        }
    }
    dirs::home_dir().map(|h| h.join(".local/share/rfm/history"))
}

impl History {
    /// Загружает историю из файла (или пустую).
    pub fn load() -> Self {
        match data_file().and_then(|p| load_from(&p)) {
            Some(entries) => Self { entries },
            None => Self::default(),
        }
    }

    /// Записывает использование команды и сохраняет (в тестах не пишет на диск).
    pub fn record(&mut self, cmd: &str) {
        self.record_at(cmd, now_secs());
        if !cfg!(test) {
            self.save();
        }
    }

    fn record_at(&mut self, cmd: &str, now: u64) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if let Some(e) = self.entries.iter_mut().find(|e| e.cmd == cmd) {
            e.count += 1;
            e.last = now;
        } else {
            self.entries.push(HistEntry {
                cmd: cmd.to_string(),
                count: 1,
                last: now,
            });
        }
    }

    /// Команды по убыванию времени последнего использования (для `Ctrl+p`/`Ctrl+n`).
    pub fn recent(&self) -> Vec<String> {
        let mut v: Vec<&HistEntry> = self.entries.iter().collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.last));
        v.into_iter().map(|e| e.cmd.clone()).collect()
    }

    /// Команды, отфильтрованные подстрокой `query`, ранжированные по частоте,
    /// затем по свежести (для диалога `Ctrl+g`).
    pub fn ranked(&self, query: &str) -> Vec<String> {
        let q = query.to_lowercase();
        let mut v: Vec<&HistEntry> = self
            .entries
            .iter()
            .filter(|e| e.cmd.to_lowercase().contains(&q))
            .collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then(b.last.cmp(&a.last)));
        v.into_iter().map(|e| e.cmd.clone()).collect()
    }

    fn save(&self) {
        if let Some(p) = data_file() {
            let _ = save_to(&p, &self.entries);
        }
    }
}

fn save_to(path: &Path, entries: &[HistEntry]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut s = String::new();
    for e in entries {
        // Команда с табами/переводами строк не поддерживается — пропускаем такие символы.
        let cmd = e.cmd.replace(['\t', '\n', '\r'], " ");
        s.push_str(&format!("{}\t{}\t{}\n", e.count, e.last, cmd));
    }
    fs::write(path, s)
}

fn load_from(path: &Path) -> Option<Vec<HistEntry>> {
    let text = fs::read_to_string(path).ok()?;
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let count = parts.next()?.trim().parse().ok()?;
        let last = parts.next()?.trim().parse().ok()?;
        let cmd = match parts.next() {
            Some(c) => c.to_string(),
            None => continue,
        };
        entries.push(HistEntry { cmd, count, last });
    }
    Some(entries)
}

#[cfg(test)]
#[path = "history_test.rs"]
mod tests;
