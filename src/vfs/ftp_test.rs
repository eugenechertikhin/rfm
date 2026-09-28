//! Тесты FTP-локации: разбор URL (листинг парсится общим `sftp::parse_ls`).

use super::*;

#[test]
fn parse_url_variants() {
    let t = parse_url("ftp://user@host/pub/data").unwrap();
    assert_eq!(t.user.as_deref(), Some("user"));
    assert_eq!(t.host, "host");
    assert_eq!(t.port, None);
    assert_eq!(t.path, "/pub/data");

    let t = parse_url("ftp://host").unwrap();
    assert_eq!(t.user, None);
    assert_eq!(t.path, "/"); // без пути → корень

    let t = parse_url("ftp://anon@ftp.example.com:2121/x").unwrap();
    assert_eq!(t.port, Some(2121));
    assert_eq!(user_host(&t), "anon@ftp.example.com");

    assert!(parse_url("sftp://x").is_none()); // чужая схема
    assert!(parse_url("ftp://").is_none());
}
