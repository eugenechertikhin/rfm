//! Автодополнение имён по `Shift+Tab` — в командной строке и в диалогах ввода.
//!
//! Берём «слово» под курсором (в командной строке — последний токен, в диалоге —
//! всю строку до курсора), делим на директорию и префикс-basename (учитывая `/`,
//! `~`, абсолютные пути), ищем записи в директории с этим префиксом (регистрозависимо).
//! Один вариант — дополняем сразу; несколько — режим выбора (`Shift+Tab`/`→`/`←`,
//! `Enter` — принять, `Esc` — отмена, печать/`Backspace` — правит строку и перестраивает).

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::CmdLine;

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
            KeyCode::Backspace => {
                cmd.backspace();
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
    let token_start = if whole_line {
        0
    } else {
        // назад до пробела
        let mut i = end;
        while i > 0 && !cmd.chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    };
    let token: String = cmd.chars[token_start..end].iter().collect();

    // Делим токен на dir-часть (до последнего `/` включительно) и префикс.
    let (dir_str, prefix, prefix_start) = match token.rfind('/') {
        Some(pos) => {
            let dir_part = &token[..=pos];
            let pref = token[pos + 1..].to_string();
            let pstart = token_start + dir_part.chars().count();
            (dir_part.to_string(), pref, pstart)
        }
        None => (String::new(), token.clone(), token_start),
    };

    let dir = resolve_dir(&dir_str, base)?;
    Some((dir, prefix, prefix_start, end))
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

/// Заменяет `[start..end)` в буфере на имя (при необходимости в кавычках; для
/// директории добавляет `/`), ставит курсор после вставки.
fn apply(cmd: &mut CmdLine, dir: &Path, start: usize, end: usize, name: &str, quote: bool) {
    let is_dir = dir.join(name).is_dir();
    let mut text = if quote && needs_quote(name) {
        quote_name(name)
    } else {
        name.to_string()
    };
    if is_dir {
        text.push('/');
    }
    let ins: Vec<char> = text.chars().collect();
    let end = end.min(cmd.chars.len());
    let start = start.min(end);
    cmd.chars.splice(start..end, ins.iter().copied());
    cmd.cursor = start + ins.len();
}

/// Нужны ли кавычки (есть пробелы/спецсимволы шелла).
fn needs_quote(s: &str) -> bool {
    s.is_empty()
        || s.chars()
            .any(|c| !(c.is_ascii_alphanumeric() || "._-+@%=:,~".contains(c)))
}

/// Одинарные кавычки с экранированием внутренних `'` (как `Ctrl+v`).
fn quote_name(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
#[path = "complete_test.rs"]
mod tests;
