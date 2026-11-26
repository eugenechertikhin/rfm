//! Автодополнение имён по `Shift+Tab` — в командной строке и в диалогах ввода.
//!
//! Берём «слово» под курсором (в командной строке — последний токен, в диалоге —
//! всю строку до курсора), делим на директорию и префикс-basename (учитывая `/`,
//! `~`, абсолютные пути), ищем записи в директории с этим префиксом (регистрозависимо).
//! Один вариант — дополняем сразу; несколько — режим выбора (`Shift+Tab`/`→`/`←`,
//! `Enter` — принять, `Esc` — отмена, печать/`Backspace`/`Delete` — правит строку и перестраивает).

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::CmdLine;
use crate::shell;

/// Состояние выбора варианта дополнения (когда их несколько).
pub struct Completion {
    pub candidates: Vec<String>,
    pub sel: usize,
}

/// Результат обработки клавиши автодополнением.
pub(super) enum CompleteKey {
    /// Клавиша поглощена; опционально — сообщение в статус.
    Consumed(Option<String>),
    /// Клавиша не относится к дополнению — обрабатывать обычным образом.
    Passthrough,
}

/// Обрабатывает клавишу для автодополнения буфера `cmd`.
/// Вызывать, когда `comp` уже активен ИЛИ клавиша — `Shift+Tab`.
pub(super) fn complete_key(
    cmd: &mut CmdLine,
    comp: &mut Option<Completion>,
    base: Option<&Path>,
    whole_line: bool,
    quote: bool,
    key: KeyEvent,
) -> CompleteKey {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    if comp.is_some() {
        match key.code {
            KeyCode::BackTab | KeyCode::Right => {
                if let Some(c) = comp.as_mut() {
                    c.sel = (c.sel + 1) % c.candidates.len();
                }
                CompleteKey::Consumed(None)
            }
            KeyCode::Left => {
                if let Some(c) = comp.as_mut() {
                    c.sel = (c.sel + c.candidates.len() - 1) % c.candidates.len();
                }
                CompleteKey::Consumed(None)
            }
            KeyCode::Enter => {
                accept(cmd, comp, base, whole_line, quote);
                CompleteKey::Consumed(None)
            }
            KeyCode::Esc => {
                *comp = None;
                CompleteKey::Consumed(None)
            }
            KeyCode::Backspace | KeyCode::Delete => {
                if key.code == KeyCode::Delete {
                    cmd.delete();
                } else {
                    cmd.backspace();
                }
                rebuild(cmd, comp, base, whole_line);
                CompleteKey::Consumed(None)
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                cmd.insert(c);
                rebuild(cmd, comp, base, whole_line);
                CompleteKey::Consumed(None)
            }
            // Прочее — выходим из режима и отдаём клавишу обычному обработчику.
            _ => {
                *comp = None;
                CompleteKey::Passthrough
            }
        }
    } else if key.code == KeyCode::BackTab {
        trigger(cmd, comp, base, whole_line, quote)
    } else {
        CompleteKey::Passthrough
    }
}

/// Запуск дополнения (`Shift+Tab`, режим ещё не активен).
fn trigger(
    cmd: &mut CmdLine,
    comp: &mut Option<Completion>,
    base: Option<&Path>,
    whole_line: bool,
    quote: bool,
) -> CompleteKey {
    let Some((dir, prefix, start, end)) = current_word(cmd, base, whole_line) else {
        return CompleteKey::Consumed(Some("no completion here".to_string()));
    };
    let cands = candidates(&dir, &prefix);
    match cands.len() {
        0 => CompleteKey::Consumed(Some(format!("no match: {prefix}"))),
        1 => {
            apply(cmd, &dir, start, end, &cands[0], quote);
            CompleteKey::Consumed(None)
        }
        _ => {
            *comp = Some(Completion { candidates: cands, sel: 0 });
            CompleteKey::Consumed(None)
        }
    }
}

/// Перестраивает список кандидатов после правки строки (печать/Backspace).
fn rebuild(cmd: &CmdLine, comp: &mut Option<Completion>, base: Option<&Path>, whole_line: bool) {
    let cands = current_word(cmd, base, whole_line)
        .map(|(dir, prefix, _, _)| candidates(&dir, &prefix))
        .unwrap_or_default();
    if cands.is_empty() {
        *comp = None;
    } else {
        *comp = Some(Completion { candidates: cands, sel: 0 });
    }
}

/// Принимает текущий выбранный вариант.
fn accept(
    cmd: &mut CmdLine,
    comp: &mut Option<Completion>,
    base: Option<&Path>,
    whole_line: bool,
    quote: bool,
) {
    if let Some(c) = comp.take() {
        let name = c.candidates[c.sel].clone();
        if let Some((dir, _, start, end)) = current_word(cmd, base, whole_line) {
            apply(cmd, &dir, start, end, &name, quote);
        }
    }
}

/// Слово под курсором → `(директория, префикс-basename, начало, конец)` в символах.
/// `None`, если дополнять не от чего (нет локальной директории).
fn current_word(
    cmd: &CmdLine,
    base: Option<&Path>,
    whole_line: bool,
) -> Option<(PathBuf, String, usize, usize)> {
    let end = cmd.cursor;
    // В командной строке слово — в синтаксисе шелла (`\ ` — не граница слова,
    // экранирование снимается); в диалогах строка — литеральный путь.
    let shell = !whole_line;
    let token_start = if whole_line { 0 } else { shell_token_start(&cmd.chars[..end]) };
    let token = &cmd.chars[token_start..end];

    // Делим токен на dir-часть (до последнего `/` включительно) и префикс.
    let slash = token.iter().rposition(|&c| c == '/');
    let (dir_raw, pref_raw) = match slash {
        Some(pos) => (&token[..=pos], &token[pos + 1..]),
        None => (&token[..0], token),
    };
    let text = |cs: &[char]| -> String {
        let s: String = cs.iter().collect();
        if shell { shell::shell_unescape(&s) } else { s }
    };
    let prefix_start = token_start + dir_raw.len();

    let dir = resolve_dir(&text(dir_raw), base)?;
    Some((dir, text(pref_raw), prefix_start, end))
}

/// Начало последнего шеллового слова: назад до пробела, не экранированного `\`.
fn shell_token_start(chars: &[char]) -> usize {
    // Проход вперёд — чтобы корректно учесть `\\` (экранированный слэш).
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            c if c.is_whitespace() => {
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    start.min(chars.len())
}

/// Разрешает dir-часть токена в путь (учитывая `/`, `~`, относительность к панели).
fn resolve_dir(dir_str: &str, base: Option<&Path>) -> Option<PathBuf> {
    if dir_str.is_empty() {
        return base.map(|p| p.to_path_buf());
    }
    if let Some(rest) = dir_str.strip_prefix("~/") {
        return dirs::home_dir().map(|h| h.join(rest));
    }
    if dir_str == "~" || dir_str == "~/" {
        return dirs::home_dir();
    }
    let p = Path::new(dir_str);
    if p.is_absolute() {
        Some(p.to_path_buf())
    } else {
        base.map(|b| b.join(dir_str))
    }
}

/// Имена записей `dir`, начинающиеся с `prefix` (регистрозависимо), отсортированные.
fn candidates(dir: &Path, prefix: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(prefix))
        .collect();
    out.sort();
    out
}

/// Заменяет `[start..end)` в буфере на имя (в командной строке — экранированное; для
/// директории добавляет `/`), ставит курсор после вставки.
fn apply(cmd: &mut CmdLine, dir: &Path, start: usize, end: usize, name: &str, quote: bool) {
    let is_dir = dir.join(name).is_dir();
    let mut text = if quote { shell::shell_escape(name) } else { name.to_string() };
    if is_dir {
        text.push('/');
    }
    let ins: Vec<char> = text.chars().collect();
    let end = end.min(cmd.chars.len());
    let start = start.min(end);
    cmd.chars.splice(start..end, ins.iter().copied());
    cmd.cursor = start + ins.len();
}


#[cfg(test)]
#[path = "complete_test.rs"]
mod tests;
