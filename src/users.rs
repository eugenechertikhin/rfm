//! Резолв числовых uid/gid в имена через libc (`getpwuid`/`getgrgid`).
//! Идём через NSS, поэтому имена видны и при LDAP/SSSD, где `/etc/passwd`
//! пустой. Результаты кэшируем. Если имя не найдено — возвращаем само число.

use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::Mutex;
use std::sync::OnceLock;

fn user_cache() -> &'static Mutex<HashMap<u32, String>> {
    static C: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn group_cache() -> &'static Mutex<HashMap<u32, String>> {
    static C: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Имя пользователя по uid через `getpwuid` (NSS). При отсутствии — число.
pub fn user_name(uid: u32) -> String {
    if let Some(n) = user_cache().lock().unwrap().get(&uid) {
        return n.clone();
    }
    let name = lookup_pw(uid).unwrap_or_else(|| uid.to_string());
    user_cache().lock().unwrap().insert(uid, name.clone());
    name
}

/// Имя группы по gid через `getgrgid` (NSS). При отсутствии — число.
pub fn group_name(gid: u32) -> String {
    if let Some(n) = group_cache().lock().unwrap().get(&gid) {
        return n.clone();
    }
    let name = lookup_gr(gid).unwrap_or_else(|| gid.to_string());
    group_cache().lock().unwrap().insert(gid, name.clone());
    name
}

/// `getpwuid(uid).pw_name`, если запись есть. Небезопасно из-за FFI и
/// разыменования указателя из статического буфера libc.
fn lookup_pw(uid: u32) -> Option<String> {
    unsafe {
        let pw = libc::getpwuid(uid as libc::uid_t);
        if pw.is_null() {
            return None;
        }
        cstr_to_string((*pw).pw_name)
    }
}

/// `getgrgid(gid).gr_name`, если запись есть.
fn lookup_gr(gid: u32) -> Option<String> {
    unsafe {
        let gr = libc::getgrgid(gid as libc::gid_t);
        if gr.is_null() {
            return None;
        }
        cstr_to_string((*gr).gr_name)
    }
}

/// Копирует C-строку в `String`; `null`/пустое — `None`.
unsafe fn cstr_to_string(p: *const libc::c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = CStr::from_ptr(p).to_string_lossy().into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Эффективный uid текущего пользователя (`getuid`).
pub fn current_uid() -> u32 {
    unsafe { libc::getuid() as u32 }
}

/// Короткое имя хоста (`gethostname`, до первой точки). При ошибке — `localhost`.
pub fn hostname() -> String {
    let mut buf = vec![0u8; 256];
    let ret = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if ret != 0 {
        return "localhost".to_string();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let full = String::from_utf8_lossy(&buf[..end]);
    let short = full.split('.').next().unwrap_or("").trim();
    if short.is_empty() {
        "localhost".to_string()
    } else {
        short.to_string()
    }
}

#[cfg(test)]
#[path = "users_test.rs"]
mod tests;
