//! HTTP(S)-локация через `curl` (shell-out). HTTP — не файловая система, поэтому
//! трактуем URL как директорию с автоиндексом: тянем HTML и парсим ссылки `<a href>`
//! в список записей (директория — ссылка с хвостовым `/`). Формат зависит от сервера —
//! это best-effort (работает для типичных autoindex: nginx/apache/python -m http.server).

use std::io;
use std::process::Command;

use super::{EntryKind, VfsEntry};

/// Разобранный http(s)-URL: origin (`scheme://host[:port]`) и путь.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpUrl {
    pub origin: String,
    pub path: String,
}

/// Разбирает `http(s)://host[:port]/path`. Без пути → `/`.
pub fn parse_url(s: &str) -> Option<HttpUrl> {
    let scheme = if s.starts_with("https://") {
        "https"
    } else if s.starts_with("http://") {
        "http"
    } else {
        return None;
    };
    let rest = &s[scheme.len() + 3..]; // после "://"
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() {
        return None;
    }
    let path = if path.is_empty() { "/".to_string() } else { path };
    Some(HttpUrl {
        origin: format!("{scheme}://{authority}"),
        path,
    })
}

/// Собирает URL директории (с хвостовым `/`).
fn dir_url(origin: &str, path: &str) -> String {
    let mut url = String::from(origin);
    if !path.starts_with('/') {
        url.push('/');
    }
    url.push_str(path);
    if !url.ends_with('/') {
        url.push('/');
    }
    url
}

/// Тянет HTML директории через `curl` и парсит автоиндекс.
pub fn list(origin: &str, path: &str) -> io::Result<Vec<VfsEntry>> {
    let url = dir_url(origin, path);
    let out = Command::new("curl")
        .arg("-sSL")
        .arg("--connect-timeout")
        .arg("10")
        .arg(&url)
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.lines().last().unwrap_or("curl http failed").trim();
        return Err(io::Error::other(msg.to_string()));
    }
    Ok(parse_autoindex(&String::from_utf8_lossy(&out.stdout)))
}

/// Парсит ссылки `<a href="...">` из HTML-автоиндекса в записи.
/// Пропускает внешние/служебные ссылки (абсолютные, `?…`, `#…`, `..`, `.`).
/// Директория — href с хвостовым `/`. Порядок сохраняется, дубли отбрасываются.
pub fn parse_autoindex(html: &str) -> Vec<VfsEntry> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("href=") {
        let start = from + rel + 5;
        let bytes = html.as_bytes();
        if start >= bytes.len() {
            break;
        }
        // значение может быть в "…", '…' или без кавычек
        let (q, vstart) = match bytes[start] {
            b'"' => (Some(b'"'), start + 1),
            b'\'' => (Some(b'\''), start + 1),
            _ => (None, start),
        };
        let end = match q {
            Some(qc) => html[vstart..].find(qc as char).map(|i| vstart + i),
            None => html[vstart..]
                .find(|c: char| c.is_whitespace() || c == '>')
                .map(|i| vstart + i),
        };
        let Some(end) = end else { break };
        let href = &html[vstart..end];
        from = end;

        // отсеиваем внешние/служебные
        if href.is_empty()
            || href.starts_with('?')
            || href.starts_with('#')
            || href.starts_with('/')
            || href.contains("://")
            || href == "../"
            || href == "./"
            || href == ".."
            || href == "."
        {
            continue;
        }
        let is_dir = href.ends_with('/');
        let raw = href.trim_end_matches('/');
        let name = percent_decode(raw);
        if name.is_empty() || name.contains('/') {
            continue; // только непосредственные дети
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        out.push(VfsEntry {
            name,
            kind: if is_dir { EntryKind::Dir } else { EntryKind::File },
            size: None,
            permissions: None,
            owner: None,
            group: None,
            mtime: None,
            executable: false,
            symlink_target: None,
            target_is_dir: is_dir,
            depth: 0,
        });
    }
    out
}

/// Минимальный percent-decode (`%20` → пробел и т.п.).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hi = (b[i + 1] as char).to_digit(16);
            let lo = (b[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
#[path = "http_test.rs"]
mod tests;
