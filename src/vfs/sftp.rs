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

/// Аргументы `ssh` для выполнения `remote_cmd` по мастер-сокету (без переавторизации).
pub fn ssh_args(sock: &Path, t: &SftpTarget, remote_cmd: &str) -> Vec<String> {
    let mut a = vec![
        "-S".to_string(),
        sock.to_string_lossy().into_owned(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
    ];
    if let Some(p) = t.port {
        a.push("-p".to_string());
        a.push(p.to_string());
    }
    a.push(user_host(t));
    a.push(remote_cmd.to_string());
    a
}

/// Выполняет команду на сервере; stdout — при успехе, первая строка stderr — ошибка.
pub fn ssh_run(sock: &Path, t: &SftpTarget, remote_cmd: &str) -> io::Result<String> {
    let out = Command::new("ssh").args(ssh_args(sock, t, remote_cmd)).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.lines().next().unwrap_or("ssh command failed").trim();
        return Err(io::Error::other(msg.to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Листинг удалённой директории по мастер-сокету (без переавторизации).
pub fn list(sock: &Path, t: &SftpTarget, path: &str) -> io::Result<Vec<VfsEntry>> {
    let out = ssh_run(sock, t, &format!("ls -la -- {}", shell_quote(path)))?;
    Ok(parse_ls(&out))
}

/// Абсолютный путь удалённой директории (`.` → домашняя): `cd -- p && pwd`.
pub fn realpath(sock: &Path, t: &SftpTarget, path: &str) -> io::Result<String> {
    let out = ssh_run(sock, t, &format!("cd -- {} && pwd", shell_quote(path)))?;
    let p = out.trim_end_matches('\n').to_string();
    if p.is_empty() {
        return Err(io::Error::other("pwd returned nothing"));
    }
    Ok(p)
}

/// Удалённая команда над путями: `tool -- 'p1' 'p2' …` (пути экранируются).
pub fn remote_cmd(tool: &str, paths: &[&str]) -> String {
    let mut s = format!("{tool} --");
    for p in paths {
        s.push(' ');
        s.push_str(&shell_quote(p));
    }
    s
}

/// Локальная команда для экрана shell: выполнить `remote_cmd` на сервере **с терминалом**
/// (`ssh -t` по мастер-сокету) — чтобы, напр., `sudo` мог спросить пароль.
pub fn tty_cmd(sock: &Path, t: &SftpTarget, remote_cmd: &str) -> String {
    let mut s = format!("ssh -t -S {}", shell_quote(&sock.to_string_lossy()));
    if let Some(p) = t.port {
        s.push_str(&format!(" -p {p}"));
    }
    s.push(' ');
    s.push_str(&shell_quote(&user_host(t)));
    s.push(' ');
    s.push_str(&shell_quote(remote_cmd));
    s
}

/// Создаёт директорию на сервере (вместе с родителями).
pub fn mkdir(sock: &Path, t: &SftpTarget, path: &str) -> io::Result<()> {
    ssh_run(sock, t, &remote_cmd("mkdir -p", &[path])).map(|_| ())
}

/// Удаляет файл/директорию на сервере (рекурсивно).
pub fn remove(sock: &Path, t: &SftpTarget, path: &str) -> io::Result<()> {
    ssh_run(sock, t, &remote_cmd("rm -rf", &[path])).map(|_| ())
}

/// Копирует внутри сервера `src` в точный путь `dest`.
pub fn copy(sock: &Path, t: &SftpTarget, src: &str, dest: &str) -> io::Result<()> {
    ssh_run(sock, t, &remote_cmd("cp -r", &[src, dest])).map(|_| ())
}

/// Переносит/переименовывает внутри сервера `src` в точный путь `dest`.
pub fn rename(sock: &Path, t: &SftpTarget, src: &str, dest: &str) -> io::Result<()> {
    ssh_run(sock, t, &remote_cmd("mv", &[src, dest])).map(|_| ())
}

/// Существующая директория на сервере?
pub fn is_dir(sock: &Path, t: &SftpTarget, path: &str) -> bool {
    ssh_run(sock, t, &remote_cmd("test -d", &[path])).is_ok()
}

/// Аргумент batch-команды `sftp`: в двойных кавычках, `\` и `"` экранируются.
pub fn sftp_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Аргументы `sftp` (batch из stdin) по мастер-сокету.
pub fn sftp_args(sock: &Path, t: &SftpTarget) -> Vec<String> {
    let mut a = vec![
        "-q".to_string(),
        "-b".to_string(),
        "-".to_string(),
        "-o".to_string(),
        format!("ControlPath={}", sock.to_string_lossy()),
    ];
    if let Some(p) = t.port {
        a.push("-P".to_string());
        a.push(p.to_string());
    }
    a.push(user_host(t));
    a
}

/// Batch-команда передачи: `put -r "local" "remote"` / `get -r "remote" "local"`.
pub fn transfer_batch(upload: bool, local: &Path, remote: &str) -> String {
    let l = sftp_quote(&local.to_string_lossy());
    let r = sftp_quote(remote);
    if upload {
        format!("put -r {l} {r}\n")
    } else {
        format!("get -r {r} {l}\n")
    }
}

/// Выполняет batch `sftp` (stdin → команды); ошибка — первая строка stderr.
fn sftp_run(sock: &Path, t: &SftpTarget, batch: &str) -> io::Result<()> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("sftp")
        .args(sftp_args(sock, t))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(batch.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.lines().next().unwrap_or("sftp transfer failed").trim();
        return Err(io::Error::other(msg.to_string()));
    }
    Ok(())
}

/// Заливает локальный файл/директорию в точный удалённый путь `remote`.
pub fn upload(sock: &Path, t: &SftpTarget, local: &Path, remote: &str) -> io::Result<()> {
    sftp_run(sock, t, &transfer_batch(true, local, remote))
}

/// Скачивает удалённый файл/директорию в точный локальный путь `local`.
pub fn download(sock: &Path, t: &SftpTarget, remote: &str, local: &Path) -> io::Result<()> {
    sftp_run(sock, t, &transfer_batch(false, local, remote))
}

/// URL сессии для поля назначения: `sftp://[user@]host[:port]/path`.
pub fn url(t: &SftpTarget, path: &str) -> String {
    let port = t.port.map(|p| format!(":{p}")).unwrap_or_default();
    let sep = if path.starts_with('/') { "" } else { "/" };
    format!("sftp://{}{port}{sep}{path}", user_host(t))
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
