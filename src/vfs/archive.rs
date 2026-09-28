//! Бэкенд архивов: матчинг типа по расширению, шелл-листинг, парсеры (zip/tar),
//! и построение виртуального дерева (непосредственные дети текущего подпути).
//!
//! Архив читается один раз в память как плоский список `ArchiveItem` (полные пути),
//! навигация внутри — виртуальная (см. `children`).

use std::io;
use std::path::Path;
use std::process::Command;

use crate::config::ArchiveConfig;
use crate::shell::shell_quote;

use super::{EntryKind, VfsEntry};

/// Одна запись архива: полный путь внутри архива и метаданные (если формат их даёт).
#[derive(Debug, Clone)]
pub struct ArchiveItem {
    /// Полный путь внутри архива, напр. `src/main.rs` (директории — без хвостового `/`).
    pub path: String,
    pub size: Option<u64>,
    pub permissions: Option<u32>,
    pub is_dir: bool,
}

/// Подбирает тип архива по имени файла: регистронезависимо, по glob-паттернам расширений.
/// Из всех совпавших побеждает самый длинный суффикс; при равенстве — первый по порядку
/// в конфиге (и первый по порядку в списке `extensions`).
pub fn match_archive<'a>(name: &str, archives: &'a [ArchiveConfig]) -> Option<&'a ArchiveConfig> {
    let lower = name.to_ascii_lowercase();
    // суффиксы после каждой точки: "a.tar.gz" → ["tar.gz", "gz"]
    let suffixes: Vec<&str> = lower
        .match_indices('.')
        .map(|(i, _)| &lower[i + 1..])
        .collect();

    let mut best: Option<(usize, usize, &ArchiveConfig)> = None; // (длина суффикса, индекс конфига, cfg)
    for (ci, cfg) in archives.iter().enumerate() {
        for pat in &cfg.extensions {
            let pat_lower = pat.to_ascii_lowercase();
            for s in &suffixes {
                if glob_match(&pat_lower, s) {
                    let len = s.len();
                    let better = match best {
                        None => true,
                        Some((blen, bci, _)) => len > blen || (len == blen && ci < bci),
                    };
                    if better {
                        best = Some((len, ci, cfg));
                    }
                }
            }
        }
    }
    best.map(|(_, _, cfg)| cfg)
}

/// glob-матч по всей строке: `*` — любое кол-во, `?` — один символ,
/// `[...]` — класс (диапазоны `a-z`, отрицание `[!...]`). Регистр уже приведён вызывающим.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_rec(&p, &t)
}

fn glob_rec(p: &[char], t: &[char]) -> bool {
    if p.is_empty() {
        return t.is_empty();
    }
    match p[0] {
        '*' => {
            // '*' поглощает 0..N символов.
            glob_rec(&p[1..], t) || (!t.is_empty() && glob_rec(p, &t[1..]))
        }
        '?' => !t.is_empty() && glob_rec(&p[1..], &t[1..]),
        '[' => {
            if t.is_empty() {
                return false;
            }
            match class_match(&p[1..], t[0]) {
                Some((matched, rest)) if matched => glob_rec(rest, &t[1..]),
                Some(_) => false,
                None => {
                    // некорректный класс — трактуем '[' как обычный символ
                    p[0] == t[0] && glob_rec(&p[1..], &t[1..])
                }
            }
        }
        c => !t.is_empty() && c == t[0] && glob_rec(&p[1..], &t[1..]),
    }
}

/// Разбирает класс `[...]` начиная сразу после `[`. Возвращает (совпал ли `ch`, хвост после `]`).
/// `None`, если закрывающего `]` нет.
fn class_match(after_bracket: &[char], ch: char) -> Option<(bool, &[char])> {
    let mut i = 0;
    let negate = after_bracket.first() == Some(&'!');
    if negate {
        i += 1;
    }
    let mut matched = false;
    let start = i;
    while i < after_bracket.len() {
        let c = after_bracket[i];
        if c == ']' && i > start {
            let rest = &after_bracket[i + 1..];
            return Some((matched ^ negate, rest));
        }
        // диапазон a-z
        if i + 2 < after_bracket.len() && after_bracket[i + 1] == '-' && after_bracket[i + 2] != ']' {
            let lo = c;
            let hi = after_bracket[i + 2];
            if lo <= ch && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if c == ch {
                matched = true;
            }
            i += 1;
        }
    }
    None // нет закрывающей ']'
}

/// Открывает архив: шеллит `list_command` + путь к файлу, парсит вывод по типу (`name`).
pub fn open(file: &Path, cfg: &ArchiveConfig) -> io::Result<Vec<ArchiveItem>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let cmd = format!("{} {}", cfg.list_command, shell_quote(&file.to_string_lossy()));
    let out = Command::new(shell).arg("-c").arg(&cmd).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.lines().next().unwrap_or("list command failed").trim();
        return Err(io::Error::other(msg.to_string()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    match cfg.name.as_str() {
        "zip" => Ok(parse_zip(&stdout)),
        "tar" => Ok(parse_tar(&stdout)),
        "7z" => Ok(parse_7z(&stdout)),
        "rar" => Ok(parse_rar(&stdout)),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no built-in parser for archive type '{other}'"),
        )),
    }
}

/// Парсер вывода `7z l`: колонки `Date Time Attr Size Compressed Name`
/// (колонка `Compressed` бывает пустой; каталог — флаг `D` в начале `Attr`).
pub fn parse_7z(stdout: &str) -> Vec<ArchiveItem> {
    parse_columned(stdout, 2, 3, |attr| attr.starts_with('D'))
}

/// Парсер вывода `unrar l`: колонки `Attributes Size Date Time Name`
/// (каталог — `d…` для unix-атрибутов или флаг `D` для windows-атрибутов).
pub fn parse_rar(stdout: &str) -> Vec<ArchiveItem> {
    parse_columned(stdout, 0, 1, |attr| attr.starts_with('d') || attr.contains('D'))
}

/// Общий парсер колоночного листинга (7z/unrar): тело между строками-разделителями,
/// имя режется по колонке `Name` из заголовка (устойчиво к пробелам в именах и пустым
/// колонкам), `attr`/`size` берутся по индексам полей, тип каталога — по `is_dir(attr)`.
fn parse_columned(
    stdout: &str,
    attr_idx: usize,
    size_idx: usize,
    is_dir: impl Fn(&str) -> bool,
) -> Vec<ArchiveItem> {
    let mut out = Vec::new();
    let mut name_col: Option<usize> = None;
    let mut in_body = false;
    for line in stdout.lines() {
        if name_col.is_none() {
            // заголовок: содержит колонки Size и Name
            if line.contains("Size") && line.contains("Name") {
                name_col = line.find("Name");
            }
            continue;
        }
        if is_separator(line) {
            in_body = !in_body;
            continue;
        }
        if !in_body {
            continue;
        }
        let col = name_col.unwrap();
        if line.len() <= col {
            continue;
        }
        let name = line[col..].trim().to_string();
        if name.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let attr = tokens.get(attr_idx).copied().unwrap_or("");
        let size = tokens.get(size_idx).and_then(|s| s.parse::<u64>().ok());
        let dir = is_dir(attr);
        push_item_full(&mut out, name, if dir { None } else { size }, None, dir);
    }
    out
}

/// Строка-разделитель таблицы: непустая, только из `-` и пробелов, содержит `---`.
fn is_separator(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.contains("---") && t.chars().all(|c| c == '-' || c == ' ')
}

/// Парсер вывода `unzip -l`:
/// ```text
///   Length      Date    Time    Name
/// ---------  ---------- -----   ----
///         0  2024-01-01 12:00   dir/
///        12  2024-01-01 12:00   dir/file.txt
/// ---------                     -------
/// ```
pub fn parse_zip(stdout: &str) -> Vec<ArchiveItem> {
    let mut out = Vec::new();
    let mut in_body = false;
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("---------") {
            // первая линия-разделитель открывает тело, вторая — закрывает
            in_body = !in_body;
            continue;
        }
        if !in_body {
            continue;
        }
        // <size> <date> <time> <name...>
        let mut it = line.split_whitespace();
        let size = it.next().and_then(|s| s.parse::<u64>().ok());
        let _date = it.next();
        let _time = it.next();
        // остаток строки после времени — имя (может содержать пробелы)
        let name = match rest_after_n_fields(line, 3) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        push_item(&mut out, name, size, None);
    }
    out
}

/// Парсер вывода `tar -tvf` (verbose). Устойчив к формату GNU tar и BSD tar (macOS):
/// - GNU: `mode owner/group size YYYY-MM-DD HH:MM name`
/// - BSD: `mode links owner group size Mon DD HH:MM name`
///
/// Якорь — токен времени `HH:MM[:SS]`: имя идёт сразу после него, размер — целочисленный
/// токен перед датой (перед ISO-датой для GNU, перед `Mon DD` для BSD).
pub fn parse_tar(stdout: &str) -> Vec<ArchiveItem> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let mode = match tokens.first() {
            Some(m) if m.len() >= 10 => *m,
            _ => continue,
        };
        let time_idx = match tokens.iter().position(|t| is_time_token(t)) {
            Some(i) => i,
            None => continue,
        };
        // размер — целочисленный токен перед датой
        let size_idx = if time_idx >= 1 && is_iso_date(tokens[time_idx - 1]) {
            time_idx.checked_sub(2) // GNU: [size, ISO-date, time]
        } else {
            time_idx.checked_sub(3) // BSD: [size, Mon, DD, time]
        };
        let size = size_idx
            .and_then(|i| tokens.get(i))
            .and_then(|s| s.parse::<u64>().ok());
        // имя — остаток строки после токена времени
        let name_raw = match rest_after_n_fields(line, time_idx + 1) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        // для симлинков отбрасываем " -> target"
        let name = name_raw.split(" -> ").next().unwrap_or(name_raw).to_string();
        let is_dir = mode.starts_with('d') || name.ends_with('/');
        let perms = parse_mode(mode);
        push_item_full(&mut out, name, if is_dir { None } else { size }, perms, is_dir);
    }
    out
}

/// Токен времени: `HH:MM` или `HH:MM:SS` (только цифры и `:`).
fn is_time_token(t: &str) -> bool {
    let parts: Vec<&str> = t.split(':').collect();
    (parts.len() == 2 || parts.len() == 3)
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// ISO-дата `YYYY-MM-DD` (формат GNU tar).
fn is_iso_date(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, &c)| {
            if i == 4 || i == 7 {
                c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Добавляет запись, нормализуя хвостовой `/` и определяя директорию по нему.
fn push_item(out: &mut Vec<ArchiveItem>, name: String, size: Option<u64>, perms: Option<u32>) {
    let is_dir = name.ends_with('/');
    push_item_full(out, name, if is_dir { None } else { size }, perms, is_dir);
}

fn push_item_full(out: &mut Vec<ArchiveItem>, name: String, size: Option<u64>, perms: Option<u32>, is_dir: bool) {
    let path = name.trim_end_matches('/').to_string();
    if path.is_empty() || path == "." {
        return;
    }
    out.push(ArchiveItem {
        path,
        size,
        permissions: perms,
        is_dir,
    });
}

/// Возвращает остаток строки после `n` полей (разделённых пробелами), сохраняя пробелы в имени.
fn rest_after_n_fields(line: &str, n: usize) -> Option<&str> {
    let mut idx = 0;
    let bytes = line.as_bytes();
    let mut fields = 0;
    // пропускаем ведущие пробелы
    while fields < n {
        while idx < bytes.len() && bytes[idx] == b' ' {
            idx += 1;
        }
        if idx >= bytes.len() {
            return None;
        }
        // поле
        while idx < bytes.len() && bytes[idx] != b' ' {
            idx += 1;
        }
        fields += 1;
    }
    while idx < bytes.len() && bytes[idx] == b' ' {
        idx += 1;
    }
    Some(&line[idx..])
}

/// Переводит строку прав вида `drwxr-xr-x` в unix-mode (9 бит прав).
fn parse_mode(mode: &str) -> Option<u32> {
    let chars: Vec<char> = mode.chars().collect();
    if chars.len() < 10 {
        return None;
    }
    let mut bits: u32 = 0;
    // 9 символов прав, начиная с индекса 1
    for (i, &c) in chars[1..10].iter().enumerate() {
        if c != '-' {
            bits |= 1 << (8 - i);
        }
    }
    Some(bits)
}

/// Строит непосредственных детей подпути `cwd` (пусто = корень архива).
/// Директории синтезируются из путей и дедуплицируются; порядок — по имени.
pub fn children(items: &[ArchiveItem], cwd: &str) -> Vec<VfsEntry> {
    use std::collections::BTreeMap;
    let prefix = if cwd.is_empty() {
        String::new()
    } else {
        format!("{cwd}/")
    };
    let mut map: BTreeMap<String, VfsEntry> = BTreeMap::new();
    for it in items {
        if !it.path.starts_with(&prefix) {
            continue;
        }
        let rest = &it.path[prefix.len()..];
        if rest.is_empty() {
            continue;
        }
        match rest.find('/') {
            // вложено глубже — непосредственный ребёнок — директория (синтезируем)
            Some(pos) => {
                let dir = rest[..pos].to_string();
                map.entry(dir.clone()).or_insert_with(|| dir_entry(&dir));
            }
            // сам ребёнок
            None => {
                if it.is_dir {
                    map.insert(rest.to_string(), dir_entry(rest));
                } else {
                    map.entry(rest.to_string())
                        .or_insert_with(|| file_entry(rest, it.size, it.permissions));
                }
            }
        }
    }
    map.into_values().collect()
}

fn dir_entry(name: &str) -> VfsEntry {
    VfsEntry {
        name: name.to_string(),
        kind: EntryKind::Dir,
        size: None,
        permissions: None,
        owner: None,
        group: None,
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: true,
        depth: 0,
    }
}

fn file_entry(name: &str, size: Option<u64>, permissions: Option<u32>) -> VfsEntry {
    VfsEntry {
        name: name.to_string(),
        kind: EntryKind::File,
        size,
        permissions,
        owner: None,
        group: None,
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: false,
        depth: 0,
    }
}

#[cfg(test)]
#[path = "archive_test.rs"]
mod tests;
