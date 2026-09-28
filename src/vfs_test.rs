//! Тесты навигации `VfsPath` по слою архива (вход/выход, снятие слоя на корне).

use super::archive::ArchiveItem;
use super::*;

fn item(path: &str, is_dir: bool) -> ArchiveItem {
    ArchiveItem {
        path: path.to_string(),
        size: None,
        permissions: None,
        is_dir,
    }
}

fn archive_path() -> VfsPath {
    let mut p = VfsPath::local(PathBuf::from("/home/user"));
    let items = vec![
        item("readme", false),
        item("src/main.rs", false),
        item("src/lib.rs", false),
    ];
    p.push_archive_items("proj.zip", items);
    p
}

#[test]
fn archive_root_lists_children() {
    let p = archive_path();
    let names: Vec<String> = p.list(true).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["readme".to_string(), "src".to_string()]);
    assert!(p.local_path().is_none()); // внутри архива нет локального пути
    assert_eq!(p.base_local_path(), Some(std::path::Path::new("/home/user")));
    assert!(p.can_go_up());
}

#[test]
fn archive_enter_and_up_within() {
    let mut p = archive_path();
    p.enter_dir("src");
    assert!(p.display().contains("proj.zip:/src"));
    let names: Vec<String> = p.list(true).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["lib.rs".to_string(), "main.rs".to_string()]);
    // выход из подпапки — курсор возвращается на "src"
    assert_eq!(p.go_up(), Some("src".to_string()));
}

#[test]
fn archive_root_up_pops_layer() {
    let mut p = archive_path();
    // на корне архива go_up снимает слой и возвращает имя архива для курсора
    assert_eq!(p.go_up(), Some("proj.zip".to_string()));
    assert_eq!(p.local_path(), Some(std::path::Path::new("/home/user")));
}

#[test]
fn sftp_path_navigation_arithmetic() {
    // Проверяем только арифметику путей слоя SFTP (без реальной сети/ssh).
    let mut p = VfsPath::local(PathBuf::from("/home/user"));
    let t = super::sftp::SftpTarget {
        user: Some("u".to_string()),
        host: "h".to_string(),
        port: None,
        path: "/var".to_string(),
    };
    p.push_sftp(t, PathBuf::from("/tmp/rfm-sock-test"), "/var".to_string());
    assert!(p.display().contains("u@h:/var"));
    assert!(p.local_path().is_none());
    assert_eq!(p.base_local_path(), Some(std::path::Path::new("/home/user")));
    assert!(p.can_go_up());

    p.enter_dir("log");
    assert!(p.display().contains("u@h:/var/log"));
    // выход из подпапки — возвращаемся к корню сессии, курсор на "log"
    assert_eq!(p.go_up(), Some("log".to_string()));
    assert!(p.display().contains("u@h:/var"));
    // теперь cwd == root (следующий go_up снял бы слой — сеть не трогаем в тесте)
}
