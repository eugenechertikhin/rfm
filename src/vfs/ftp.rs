//! FTP-локация через `curl` (shell-out). Persistent-соединения у FTP нет, поэтому
//! пароль спрашивается один раз и хранится в памяти слоя, подставляясь в каждый `curl`.
//! Листинг (`curl ftp://host/dir/`) — это серверный LIST в формате `ls -l`; парсим
//! общим парсером [`crate::vfs::sftp::parse_ls`].

use std::io;
use std::process::Command;

use super::VfsEntry;

/// Разобранный `ftp://[user@]host[:port]/path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtpTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
}

/// Разбирает `ftp://[user@]host[:port]/path`. Без пути → `/`.
pub fn parse_url(s: &str) -> Option<FtpTarget> {
    let rest = s.strip_prefix("ftp://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
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
    let path = if path.is_empty() { "/".to_string() } else { path };
    Some(FtpTarget { user, host, port, path })
}

/// Собирает URL директории (с хвостовым `/` — иначе curl скачает как файл).
fn dir_url(t: &FtpTarget, path: &str) -> String {
    let mut url = String::from("ftp://");
    url.push_str(&t.host);
    if let Some(p) = t.port {
        url.push(':');
        url.push_str(&p.to_string());
    }
    if !path.starts_with('/') {
        url.push('/');
    }
    url.push_str(path);
    if !url.ends_with('/') {
        url.push('/');
    }
    url
}

/// Листинг удалённой директории через `curl` (LIST → `ls -l` → `VfsEntry`).
pub fn list(t: &FtpTarget, password: Option<&str>, path: &str) -> io::Result<Vec<VfsEntry>> {
    let url = dir_url(t, path);
    let mut cmd = Command::new("curl");
    cmd.arg("-sS").arg("--connect-timeout").arg("10");
    if let Some(u) = &t.user {
        let auth = match password {
            Some(pw) => format!("{u}:{pw}"),
            None => u.clone(),
        };
        cmd.arg("-u").arg(auth);
    }
    cmd.arg(&url);
    let out = cmd.output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.lines().last().unwrap_or("curl ftp failed").trim();
        return Err(io::Error::other(msg.to_string()));
    }
    Ok(crate::vfs::sftp::parse_ls(&String::from_utf8_lossy(&out.stdout)))
}

/// `user@host` либо `host` для заголовка панели.
pub fn user_host(t: &FtpTarget) -> String {
    match &t.user {
        Some(u) => format!("{u}@{}", t.host),
        None => t.host.clone(),
    }
}

#[cfg(test)]
#[path = "ftp_test.rs"]
mod tests;
