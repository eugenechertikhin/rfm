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

// ---- Команды операций по мастер-сокету ----

fn tgt(port: Option<u16>) -> SftpTarget {
    SftpTarget { user: Some("u".into()), host: "h".into(), port, path: "/".into() }
}

#[test]
fn ssh_args_use_socket_batchmode_and_port() {
    let a = ssh_args(Path::new("/s"), &tgt(Some(2222)), "ls");
    assert_eq!(a, ["-S", "/s", "-o", "BatchMode=yes", "-p", "2222", "u@h", "ls"]);
    let a = ssh_args(Path::new("/s"), &tgt(None), "ls");
    assert!(!a.contains(&"-p".to_string()));
}

#[test]
fn remote_cmd_quotes_paths() {
    assert_eq!(remote_cmd("mv", &["/a b", "it's"]), r"mv -- '/a b' 'it'\''s'");
    assert_eq!(remote_cmd("mkdir -p", &["/x"]), "mkdir -p -- '/x'");
}

#[test]
fn sftp_batch_quoting_and_direction() {
    assert_eq!(sftp_quote(r#"a "b"\c"#), r#""a \"b\"\\c""#);
    assert_eq!(transfer_batch(true, Path::new("/l f"), "/r"), "put -r \"/l f\" \"/r\"\n");
    assert_eq!(transfer_batch(false, Path::new("/l"), "/r x"), "get -r \"/r x\" \"/l\"\n");
}

#[test]
fn sftp_args_use_control_path_and_capital_p() {
    let a = sftp_args(Path::new("/s"), &tgt(Some(22)));
    assert_eq!(a, ["-q", "-b", "-", "-o", "ControlPath=/s", "-P", "22", "u@h"]);
}

#[test]
fn url_formats_session() {
    assert_eq!(url(&tgt(None), "/var/x"), "sftp://u@h/var/x");
    assert_eq!(url(&tgt(Some(2222)), "rel"), "sftp://u@h:2222/rel");
    // URL разбирается обратно в ту же цель
    let t = parse_url(&url(&tgt(Some(2222)), "/var")).unwrap();
    assert_eq!((t.port, t.path.as_str()), (Some(2222), "/var"));
}
