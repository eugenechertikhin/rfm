//! Тесты бэкенда архивов: glob-матчинг, выбор типа, парсеры, виртуальное дерево.

use super::*;
use crate::config::ArchiveConfig;

fn cfg(name: &str, exts: &[&str]) -> ArchiveConfig {
    ArchiveConfig {
        name: name.to_string(),
        extensions: exts.iter().map(|s| s.to_string()).collect(),
        list_command: String::new(),
    }
}

#[test]
fn glob_star_question_class() {
    assert!(glob_match("zip", "zip"));
    assert!(!glob_match("zip", "zap"));
    assert!(glob_match("z?p", "zip"));
    assert!(glob_match("z*", "zippppp"));
    assert!(glob_match("z[0-9][0-9]", "z01"));
    assert!(!glob_match("z[0-9][0-9]", "zaa"));
    assert!(glob_match("*.gz", "a.gz"));
    assert!(glob_match("[!0-9]", "a"));
    assert!(!glob_match("[!0-9]", "5"));
}

#[test]
fn match_longest_suffix_wins() {
    // "gz" и "tar.gz" оба совпадают на .tar.gz → побеждает более длинный суффикс.
    let archives = vec![cfg("gz", &["gz"]), cfg("targz", &["tar.gz"])];
    let m = match_archive("backup.tar.gz", &archives).unwrap();
    assert_eq!(m.name, "targz");
}

#[test]
fn match_tie_breaks_by_config_order() {
    let archives = vec![cfg("first", &["zip"]), cfg("second", &["zip"])];
    let m = match_archive("a.zip", &archives).unwrap();
    assert_eq!(m.name, "first");
}

#[test]
fn match_case_insensitive_and_none() {
    let archives = crate::config::builtin_archives();
    assert_eq!(match_archive("Photo.ZIP", &archives).unwrap().name, "zip");
    assert_eq!(match_archive("a.TGZ", &archives).unwrap().name, "tar");
    assert_eq!(match_archive("a.tar.gz", &archives).unwrap().name, "tar");
    assert!(match_archive("notes.txt", &archives).is_none());
}

#[test]
fn parse_zip_sample() {
    let out = "\
Archive:  test.zip
  Length      Date    Time    Name
---------  ---------- -----   ----
        0  2024-01-01 12:00   dir/
       12  2024-01-01 12:00   dir/file.txt
---------                     -------
       12                     2 files
";
    let items = parse_zip(out);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].path, "dir");
    assert!(items[0].is_dir);
    assert_eq!(items[0].size, None);
    assert_eq!(items[1].path, "dir/file.txt");
    assert_eq!(items[1].size, Some(12));
    assert!(!items[1].is_dir);
}

#[test]
fn parse_tar_sample() {
    let out = "\
drwxr-xr-x user/group        0 2024-01-01 12:00 dir/
-rw-r--r-- user/group       12 2024-01-01 12:00 dir/file.txt
lrwxrwxrwx user/group        0 2024-01-01 12:00 link -> target
";
    let items = parse_tar(out);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].path, "dir");
    assert!(items[0].is_dir);
    assert_eq!(items[0].permissions, Some(0o755));
    assert_eq!(items[1].path, "dir/file.txt");
    assert_eq!(items[1].size, Some(12));
    assert_eq!(items[1].permissions, Some(0o644));
    // симлинк: имя без " -> target"
    assert_eq!(items[2].path, "link");
}

#[test]
fn parse_tar_bsd_sample() {
    // формат bsdtar (macOS): mode links owner group size Mon DD HH:MM name
    let out = "\
drwxr-xr-x  0 user staff       0 Sep 15 15:32 proj/
lrwxr-xr-x  0 user staff       0 Sep 15 15:32 proj/link -> readme.txt
-rw-r--r--  0 user staff       6 Sep 15 15:32 proj/readme.txt
-rw-r--r--  0 user staff      12 Sep 15 15:32 proj/src/main.rs
";
    let items = parse_tar(out);
    assert_eq!(items.len(), 4);
    assert_eq!(items[0].path, "proj");
    assert!(items[0].is_dir);
    assert_eq!(items[1].path, "proj/link"); // симлинк без " -> ..."
    assert_eq!(items[2].path, "proj/readme.txt");
    assert_eq!(items[2].size, Some(6));
    assert_eq!(items[3].path, "proj/src/main.rs");
    assert_eq!(items[3].size, Some(12));
    assert_eq!(items[3].permissions, Some(0o644));
}

#[test]
fn match_7z_and_rar_including_multivolume() {
    let a = crate::config::builtin_archives();
    assert_eq!(match_archive("data.7z", &a).unwrap().name, "7z");
    assert_eq!(match_archive("movie.rar", &a).unwrap().name, "rar");
    assert_eq!(match_archive("disk.r01", &a).unwrap().name, "rar"); // многотомный
    assert_eq!(match_archive("disk.R09", &a).unwrap().name, "rar"); // регистр
    assert!(match_archive("disk.r0x", &a).is_none()); // не две цифры
}

// строка 7z-листинга по фиксированному шаблону (гарантирует выравнивание колонки Name)
fn r7z(dt: &str, attr: &str, size: &str, comp: &str, name: &str) -> String {
    format!("{dt:<19} {attr:<5} {size:>12} {comp:>12}  {name}")
}
// строка unrar-листинга по фиксированному шаблону
fn rrar(attr: &str, size: &str, dt: &str, name: &str) -> String {
    format!(" {attr:<11} {size:>9}  {dt:<15}  {name}")
}

#[test]
fn parse_7z_sample() {
    let sep = "------------------- ----- ------------ ------------  ----";
    let out = [
        r7z("Date Time", "Attr", "Size", "Compressed", "Name"),
        sep.to_string(),
        r7z("2020-01-01 00:00:00", "D....", "0", "0", "proj"),
        r7z("2020-01-01 00:00:00", "....A", "6", "22", "proj/readme.txt"),
        // пустая колонка Compressed (solid-блок)
        r7z("2020-01-01 00:00:00", "....A", "12", "", "proj/main.rs"),
        sep.to_string(),
    ]
    .join("\n");
    let items = parse_7z(&out);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].path, "proj");
    assert!(items[0].is_dir);
    assert_eq!(items[1].path, "proj/readme.txt");
    assert_eq!(items[1].size, Some(6));
    assert_eq!(items[2].path, "proj/main.rs"); // имя корректно при пустом Compressed
    assert_eq!(items[2].size, Some(12));
}

#[test]
fn parse_rar_sample() {
    let sep = "----------- --------- ---------------  ----";
    let out = [
        rrar("Attributes", "Size", "Date    Time", "Name"),
        sep.to_string(),
        rrar(".CA....", "222", "2019-08-06 03:53", "proj/readme.txt"),
        rrar(".C.D...", "0", "2019-08-06 03:54", "proj"), // каталог: флаг D
        sep.to_string(),
    ]
    .join("\n");
    let items = parse_rar(&out);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].path, "proj/readme.txt");
    assert_eq!(items[0].size, Some(222));
    assert!(!items[0].is_dir);
    assert_eq!(items[1].path, "proj");
    assert!(items[1].is_dir);
}

fn item(path: &str, is_dir: bool, size: Option<u64>) -> ArchiveItem {
    ArchiveItem {
        path: path.to_string(),
        size,
        permissions: None,
        is_dir,
    }
}

#[test]
fn children_root_and_subdir() {
    let items = vec![
        item("readme", false, Some(5)),
        item("src/main.rs", false, Some(10)),
        item("src/lib.rs", false, Some(20)),
    ];
    // корень: файл readme + синтезированная директория src
    let root = children(&items, "");
    let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["readme", "src"]); // BTreeMap → по имени
    assert!(root.iter().find(|e| e.name == "src").unwrap().is_dir());
    // внутри src: два файла
    let sub = children(&items, "src");
    let sub_names: Vec<&str> = sub.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(sub_names, vec!["lib.rs", "main.rs"]);
}

#[test]
fn merge_adds_missing_archivers_and_signals_change() {
    // конфиг, созданный до появления [[archive]] — архивов нет
    let mut cfg: crate::config::Config = toml::from_str("theme = \"default\"\n").unwrap();
    assert!(cfg.archives.is_empty());
    let changed = crate::config::merge_archive_defaults(&mut cfg);
    assert!(changed);
    // добавлены все дефолтные секции (zip + три tar)
    assert!(cfg.archives.iter().any(|a| a.name == "zip"));
    assert_eq!(cfg.archives.iter().filter(|a| a.name == "tar").count(), 3);
}

#[test]
fn merge_keeps_user_configured_archiver() {
    // пользователь завёл zip по-своему; tar не заведён вовсе
    let mut cfg = crate::config::Config {
        archives: vec![ArchiveConfig {
            name: "zip".to_string(),
            extensions: vec!["zip".to_string()],
            list_command: "myunzip -l".to_string(),
        }],
        ..crate::config::Config::default()
    };
    let changed = crate::config::merge_archive_defaults(&mut cfg);
    assert!(changed); // tar добавлен
    // zip не тронут (по-прежнему один, с пользовательской командой)
    let zips: Vec<_> = cfg.archives.iter().filter(|a| a.name == "zip").collect();
    assert_eq!(zips.len(), 1);
    assert_eq!(zips[0].list_command, "myunzip -l");
    // tar-семейство добавлено
    assert_eq!(cfg.archives.iter().filter(|a| a.name == "tar").count(), 3);
}

#[test]
fn merge_noop_when_all_present() {
    let mut cfg = crate::config::builtin();
    let changed = crate::config::merge_archive_defaults(&mut cfg);
    assert!(!changed); // zip и tar уже есть — ничего не добавили
}

#[test]
fn builtin_config_archives_roundtrip() {
    let cfg = crate::config::builtin();
    assert!(!cfg.archives.is_empty());
    let text = toml::to_string_pretty(&cfg).unwrap();
    assert!(text.contains("[[archive]]")); // ключ секции — singular
    let back: crate::config::Config = toml::from_str(&text).unwrap();
    assert_eq!(back.archives.len(), cfg.archives.len());
    assert_eq!(back.archives[0].name, "zip");
}
