//! Тесты SFTP-локации: разбор URL и парсинг `ls -la` (без реальной сети).

use super::*;

#[test]
fn parse_url_variants() {
    let t = parse_url("sftp://user@host/var/log").unwrap();
    assert_eq!(t.user.as_deref(), Some("user"));
    assert_eq!(t.host, "host");
    assert_eq!(t.port, None);
    assert_eq!(t.path, "/var/log");

    let t = parse_url("sftp://host").unwrap();
    assert_eq!(t.user, None);
    assert_eq!(t.host, "host");
    assert_eq!(t.path, "."); // без пути → домашняя

    let t = parse_url("sftp://me@srv:2222/x").unwrap();
    assert_eq!(t.port, Some(2222));
    assert_eq!(t.path, "/x");
    assert_eq!(user_host(&t), "me@srv");

    let t = parse_url("sftp://host/").unwrap();
    assert_eq!(t.path, "/");

    assert!(parse_url("ftp://x").is_none());
    assert!(parse_url("sftp://").is_none());
}

#[test]
fn parse_ls_skips_dot_and_parses_fields() {
    let out = "\
total 24
drwxr-xr-x  5 user group  4096 Jan  1 12:00 .
drwxr-xr-x  3 user group  4096 Jan  1 12:00 ..
-rw-r--r--  1 user group   123 Jan  1 12:00 file.txt
drwxr-xr-x  2 user group  4096 Feb 10  2020 old dir
lrwxrwxrwx  1 user group     7 Mar  3 09:15 link -> target
";
    let items = parse_ls(out);
    // '.', '..' и 'total' отброшены → 3 записи
    assert_eq!(items.len(), 3);

    let f = &items[0];
    assert_eq!(f.name, "file.txt");
    assert_eq!(f.kind, EntryKind::File);
    assert_eq!(f.size, Some(123));
    assert_eq!(f.permissions, Some(0o644));

    let d = &items[1];
    assert_eq!(d.name, "old dir"); // пробел в имени + дата с годом
    assert_eq!(d.kind, EntryKind::Dir);
    assert_eq!(d.size, None);

    let l = &items[2];
    assert_eq!(l.name, "link"); // симлинк без " -> target"
    assert_eq!(l.kind, EntryKind::Symlink);
    assert_eq!(l.symlink_target.as_deref(), Some("target"));
    assert_eq!(d.symlink_target, None);
}

#[test]
fn parse_ls_arrow_in_regular_file_name_kept() {
    let out = "-rw-r--r--  1 user group 5 Mar  3 09:15 a -> b\n";
    let items = parse_ls(out);
    assert_eq!(items[0].name, "a -> b"); // не симлинк — имя целиком
    assert_eq!(items[0].symlink_target, None);
}

#[test]
fn master_args_include_socket_and_port() {
    let t = SftpTarget {
        user: Some("u".to_string()),
        host: "h".to_string(),
        port: Some(2200),
        path: "/x".to_string(),
    };
    let args = master_args(std::path::Path::new("/tmp/sock"), &t);
    assert!(args.iter().any(|a| a == "-M"));
    assert!(args.iter().any(|a| a == "/tmp/sock"));
    assert!(args.windows(2).any(|w| w[0] == "-p" && w[1] == "2200"));
    assert_eq!(args.last().unwrap(), "u@h");
}
