//! SFTP-локация через OpenSSH ControlMaster (shell-out).
//!
//! Живого соединения в Rust нет — держим лишь путь control-сокета. Мастер поднимается
//! один раз интерактивно (`ssh -M`, пароль/known_hosts спрашивает сам ssh), а листинг и
//! навигация идут по сокету (`ssh -S sock … ls -la`) без переавторизации. Формат `ls -l`
//! разбираем устойчиво (якорь по дате `Mon DD HH:MM` или `Mon DD YYYY`).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{EntryKind, VfsEntry};
use crate::shell::shell_quote;

/// Разобранный `sftp://[user@]host[:port]/path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SftpTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
}

/// Разбирает `sftp://[user@]host[:port]/path`. Без пути → `.` (домашняя директория).
pub fn parse_url(s: &str) -> Option<SftpTarget> {
    let rest = s.strip_prefix("sftp://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, ".".to_string()),
    };
    if authority.is_empty() {
        return None;
    }
    let (user, hostport) = match authority.rfind('@') {
        Some(i) => (Some(authority[..i].to_string()), &authority[i + 1..]),
        None => (None, authority),
    };
    let (host, port) = match hostport.rfind(':') {
        Some(i) => match hostport[i + 1..].parse::<u16>() {
            Ok(p) => (hostport[..i].to_string(), Some(p)),
            Err(_) => (hostport.to_string(), None),
        },
        None => (hostport.to_string(), None),
    };
    if host.is_empty() {
        return None;
    }
    let path = if path.is_empty() { ".".to_string() } else { path };
    Some(SftpTarget { user, host, port, path })
}

/// `user@host` либо `host` для передачи ssh.
pub fn user_host(t: &SftpTarget) -> String {
    match &t.user {
        Some(u) => format!("{u}@{}", t.host),
        None => t.host.clone(),
    }
}

static SOCK_N: AtomicU32 = AtomicU32::new(0);

/// Уникальный путь control-сокета. Кладём в `/tmp` (короткий путь — лимит unix-сокета ~104).
pub fn new_sock() -> PathBuf {
    let n = SOCK_N.fetch_add(1, Ordering::SeqCst);
    PathBuf::from(format!("/tmp/rfm-ssh-{}-{}", std::process::id(), n))
}

/// Аргументы `ssh` для поднятия мастера (интерактивно: пароль/known_hosts спрашивает ssh).
pub fn master_args(sock: &Path, t: &SftpTarget) -> Vec<String> {
    let mut a = vec![
        "-M".to_string(),
        "-S".to_string(),
        sock.to_string_lossy().into_owned(),
        "-N".to_string(),
        "-f".to_string(),
        "-o".to_string(),
        "ControlPersist=300".to_string(),
    ];
    if let Some(p) = t.port {
        a.push("-p".to_string());
        a.push(p.to_string());
    }
    a.push(user_host(t));
    a
}

/// Листинг удалённой директории по мастер-сокету (без переавторизации).
pub fn list(sock: &Path, t: &SftpTarget, path: &str) -> io::Result<Vec<VfsEntry>> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-S").arg(sock).arg("-o").arg("BatchMode=yes");
    if let Some(p) = t.port {
        cmd.arg("-p").arg(p.to_string());
    }
    cmd.arg(user_host(t));
    cmd.arg(format!("ls -la -- {}", shell_quote(path)));
    let out = cmd.output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.lines().next().unwrap_or("ssh ls failed").trim();
        return Err(io::Error::other(msg.to_string()));
    }
    Ok(parse_ls(&String::from_utf8_lossy(&out.stdout)))
}

/// Закрывает мастер-соединение (best-effort).
pub fn exit_master(sock: &Path, t: &SftpTarget) {
    let mut cmd = Command::new("ssh");
    cmd.arg("-S").arg(sock).arg("-O").arg("exit");
    if let Some(p) = t.port {
        cmd.arg("-p").arg(p.to_string());
    }
    cmd.arg(user_host(t));
    let _ = cmd.output();
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn is_month(s: &str) -> bool {
    MONTHS.contains(&s)
}
fn is_num(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}
fn is_time(s: &str) -> bool {
    let p: Vec<&str> = s.split(':').collect();
    p.len() == 2 && p.iter().all(|x| is_num(x))
}
fn is_year(s: &str) -> bool {
    s.len() == 4 && is_num(s)
}

/// Разбирает вывод `ls -la`. Якорь — тройка `Month Day (HH:MM|YYYY)`: имя идёт после неё,
/// размер — поле перед месяцем. Работает для GNU и BSD `ls`. `.`/`..` пропускаются.
pub fn parse_ls(stdout: &str) -> Vec<VfsEntry> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 8 {
            continue;
        }
        let mode = tokens[0];
        if mode.len() < 10 {
            continue; // не строка листинга (напр. "total 24")
        }
        // индекс поля-времени/года
        let mut k = None;
        for i in 2..tokens.len() {
            if is_month(tokens[i - 2]) && is_num(tokens[i - 1]) && (is_time(tokens[i]) || is_year(tokens[i])) {
                k = Some(i);
                break;
            }
        }
        let k = match k {
            Some(k) if k >= 3 => k,
            _ => continue,
        };
        let size = tokens.get(k - 3).and_then(|s| s.parse::<u64>().ok());
        // Между mode(0) и size(k-3): links, owner, group. owner=k-5, group=k-4.
        let owner = if k >= 5 { tokens.get(k - 5).map(|s| s.to_string()) } else { None };
        let group = if k >= 4 { tokens.get(k - 4).map(|s| s.to_string()) } else { None };
        let name_raw = match rest_after_n_fields(line, k + 1) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let kind = match mode.as_bytes()[0] {
            b'd' => EntryKind::Dir,
            b'l' => EntryKind::Symlink,
            b'-' => EntryKind::File,
            _ => EntryKind::Other,
        };
        // симлинк: "name -> target" (только для `l` — у обычного файла " -> " может быть в имени)
        let (name, symlink_target) = match name_raw.split_once(" -> ") {
            Some((n, t)) if kind == EntryKind::Symlink => (n.to_string(), Some(t.to_string())),
            _ => (name_raw.to_string(), None),
        };
        if name == "." || name == ".." {
            continue;
        }
        let perms = parse_mode(mode);
        let is_dir = kind == EntryKind::Dir;
        let executable =
            !is_dir && kind == EntryKind::File && perms.map(|m| m & 0o111 != 0).unwrap_or(false);
        out.push(VfsEntry {
            name,
            kind,
            size: if is_dir { None } else { size },
            permissions: perms,
            owner,
            group,
            mtime: None,
            executable,
            symlink_target,
            target_is_dir: false,
            depth: 0,
        });
    }
    out
}

/// Остаток строки после `n` полей (разделённых пробелами), сохраняя пробелы в имени.
fn rest_after_n_fields(line: &str, n: usize) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut idx = 0;
    let mut fields = 0;
    while fields < n {
        while idx < bytes.len() && bytes[idx] == b' ' {
            idx += 1;
        }
        if idx >= bytes.len() {
            return None;
        }
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

/// Строка прав `drwxr-xr-x` → unix-mode (9 бит).
fn parse_mode(mode: &str) -> Option<u32> {
    let chars: Vec<char> = mode.chars().collect();
    if chars.len() < 10 {
        return None;
    }
    let mut bits: u32 = 0;
    for (i, &c) in chars[1..10].iter().enumerate() {
        if c != '-' {
            bits |= 1 << (8 - i);
        }
    }
    Some(bits)
}

#[cfg(test)]
#[path = "sftp_test.rs"]
mod tests;
