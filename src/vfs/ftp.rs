//! FTP-локация через `curl` (shell-out). Persistent-соединения у FTP нет, поэтому
//! пароль спрашивается один раз и хранится в памяти слоя, подставляясь в каждый `curl`.
//! Листинг (`curl ftp://host/dir/`) — это серверный LIST в формате `ls -l`; парсим
//! общим парсером [`crate::vfs::sftp::parse_ls`].

use std::io;
use std::path::Path;
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

/// Percent-encoding пути для URL (`/` сохраняется; безопасные — буквы/цифры/`-._~`).
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// URL файла на сервере: `ftp://host[:port]/path` (путь percent-encoded).
pub fn file_url(t: &FtpTarget, path: &str) -> String {
    let mut url = String::from("ftp://");
    url.push_str(&t.host);
    if let Some(p) = t.port {
        url.push(':');
        url.push_str(&p.to_string());
    }
    if !path.starts_with('/') {
        url.push('/');
    }
    url.push_str(&encode_path(path));
    url
}

/// URL директории (с хвостовым `/` — иначе curl скачает как файл).
fn dir_url(t: &FtpTarget, path: &str) -> String {
    let mut url = file_url(t, path);
    if !url.ends_with('/') {
        url.push('/');
    }
    url
}

/// Базовые аргументы `curl`: тихо, с ошибками, таймаут, `-u user[:pass]`.
pub fn curl_base(t: &FtpTarget, password: Option<&str>) -> Vec<String> {
    let mut a = vec!["-sS".to_string(), "--connect-timeout".to_string(), "10".to_string()];
    if let Some(u) = &t.user {
        let auth = match password {
            Some(pw) => format!("{u}:{pw}"),
            None => u.clone(),
        };
        a.push("-u".to_string());
        a.push(auth);
    }
    a
}

/// Запускает `curl` с базовыми аргументами + `extra`; stdout при успехе.
fn curl_run(t: &FtpTarget, password: Option<&str>, extra: &[String]) -> io::Result<String> {
    let out = Command::new("curl").args(curl_base(t, password)).args(extra).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.lines().last().unwrap_or("curl ftp failed").trim();
        return Err(io::Error::other(msg.to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Листинг удалённой директории через `curl` (LIST → `ls -l` → `VfsEntry`).
pub fn list(t: &FtpTarget, password: Option<&str>, path: &str) -> io::Result<Vec<VfsEntry>> {
    let out = curl_run(t, password, &[dir_url(t, path)])?;
    Ok(crate::vfs::sftp::parse_ls(&out))
}

/// Аргументы для FTP-команд (`-Q`) на корне сервера; вывод листинга отбрасывается.
/// Команда с префиксом `*` — её ошибка игнорируется (curl).
pub fn quote_args(t: &FtpTarget, cmds: &[String]) -> Vec<String> {
    let mut a = Vec::new();
    for c in cmds {
        a.push("-Q".to_string());
        a.push(c.clone());
    }
    a.push("-o".to_string());
    a.push("/dev/null".to_string());
    a.push(dir_url(t, "/"));
    a
}

fn quote(t: &FtpTarget, password: Option<&str>, cmds: &[String]) -> io::Result<()> {
    curl_run(t, password, &quote_args(t, cmds)).map(|_| ())
}

/// FTP-команды `MKD` для каждого префикса пути (существующие — без ошибки), кроме
/// последнего: его ошибка — реальная.
pub fn mkdir_cmds(path: &str) -> Vec<String> {
    let abs = path.starts_with('/');
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut cmds = Vec::new();
    for i in 0..parts.len() {
        let p = format!("{}{}", if abs { "/" } else { "" }, parts[..=i].join("/"));
        let ignore = if i + 1 < parts.len() { "*" } else { "" };
        cmds.push(format!("{ignore}MKD {p}"));
    }
    cmds
}

/// Создаёт директорию (с недостающими родителями).
pub fn mkdir(t: &FtpTarget, password: Option<&str>, path: &str) -> io::Result<()> {
    quote(t, password, &mkdir_cmds(path))
}

/// Переименование/перенос внутри сервера (`RNFR`/`RNTO`).
pub fn rename(t: &FtpTarget, password: Option<&str>, src: &str, dest: &str) -> io::Result<()> {
    quote(t, password, &[format!("RNFR {src}"), format!("RNTO {dest}")])
}

/// Удаляет файл (`DELE`) или директорию рекурсивно (содержимое, затем `RMD`).
pub fn remove(t: &FtpTarget, password: Option<&str>, path: &str, is_dir: bool) -> io::Result<()> {
    if !is_dir {
        return quote(t, password, &[format!("DELE {path}")]);
    }
    for e in list(t, password, path)? {
        remove(t, password, &super::remote_join(path, &e.name), e.is_dir())?;
    }
    quote(t, password, &[format!("RMD {path}")])
}

/// Существующая директория (листинг удался)?
pub fn is_dir(t: &FtpTarget, password: Option<&str>, path: &str) -> bool {
    list(t, password, path).is_ok()
}

/// Заливает локальный файл/директорию (рекурсивно) в точный удалённый путь.
pub fn upload(t: &FtpTarget, password: Option<&str>, local: &Path, remote: &str) -> io::Result<()> {
    let meta = std::fs::metadata(local)?;
    if meta.is_dir() {
        mkdir(t, password, remote)?;
        for e in std::fs::read_dir(local)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            upload(t, password, &e.path(), &super::remote_join(remote, &name))?;
        }
        Ok(())
    } else {
        let args = vec![
            "--ftp-create-dirs".to_string(),
            "-T".to_string(),
            local.to_string_lossy().into_owned(),
            file_url(t, remote),
        ];
        curl_run(t, password, &args).map(|_| ())
    }
}

/// Скачивает удалённый файл/директорию (рекурсивно) в точный локальный путь.
pub fn download(t: &FtpTarget, password: Option<&str>, remote: &str, is_dir: bool, local: &Path) -> io::Result<()> {
    if is_dir {
        std::fs::create_dir_all(local)?;
        for e in list(t, password, remote)? {
            download(t, password, &super::remote_join(remote, &e.name), e.is_dir(), &local.join(&e.name))?;
        }
        Ok(())
    } else {
        let args = vec![
            "-o".to_string(),
            local.to_string_lossy().into_owned(),
            file_url(t, remote),
        ];
        curl_run(t, password, &args).map(|_| ())
    }
}

/// URL сессии для поля назначения: `ftp://[user@]host[:port]/path`.
pub fn url(t: &FtpTarget, path: &str) -> String {
    let port = t.port.map(|p| format!(":{p}")).unwrap_or_default();
    let sep = if path.starts_with('/') { "" } else { "/" };
    format!("ftp://{}{port}{sep}{path}", user_host(t))
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
