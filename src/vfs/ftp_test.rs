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

// ---- URL и FTP-команды операций ----

fn tgt(user: Option<&str>) -> FtpTarget {
    FtpTarget { user: user.map(String::from), host: "h".into(), port: Some(21), path: "/".into() }
}

#[test]
fn file_url_percent_encodes_path() {
    assert_eq!(file_url(&tgt(None), "/a b/ж#.txt"), "ftp://h:21/a%20b/%D0%B6%23.txt");
    assert_eq!(dir_url(&tgt(None), "/pub"), "ftp://h:21/pub/");
    assert_eq!(dir_url(&tgt(None), "/"), "ftp://h:21/");
}

#[test]
fn curl_base_auth() {
    assert!(!curl_base(&tgt(None), None).contains(&"-u".to_string()));
    let a = curl_base(&tgt(Some("bob")), Some("pw"));
    assert_eq!(&a[a.len() - 2..], ["-u", "bob:pw"]);
}

#[test]
fn mkdir_cmds_ignore_intermediate_errors() {
    assert_eq!(mkdir_cmds("/a/b/c"), ["*MKD /a", "*MKD /a/b", "MKD /a/b/c"]);
    assert_eq!(mkdir_cmds("x"), ["MKD x"]);
}

#[test]
fn quote_args_discard_listing() {
    let a = quote_args(&tgt(None), &["RNFR /a".into(), "RNTO /b".into()]);
    assert_eq!(a, ["-Q", "RNFR /a", "-Q", "RNTO /b", "-o", "/dev/null", "ftp://h:21/"]);
}

#[test]
fn session_url_roundtrip() {
    let u = url(&tgt(Some("bob")), "/pub");
    assert_eq!(u, "ftp://bob@h:21/pub");
    assert_eq!(parse_url(&u).unwrap().path, "/pub");
}
