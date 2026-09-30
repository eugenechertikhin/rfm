//! Юнит-тесты модуля `ops` 

use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Уникальная временная директория (без внешних крейтов).
fn tmp_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rfm_test_{}_{}", std::process::id(), n));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn create_dir_makes_nested() {
    let root = tmp_dir();
    let target = root.join("a/b/c");
    create_dir(&target).unwrap();
    assert!(target.is_dir());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn delete_file_and_dir() {
    let root = tmp_dir();
    let f = root.join("f.txt");
    fs::write(&f, b"x").unwrap();
    assert!(f.exists());
    delete_path(&f).unwrap();
    assert!(!f.exists());

    let d = root.join("sub");
    fs::create_dir_all(d.join("inner")).unwrap();
    fs::write(d.join("inner/a"), b"y").unwrap();
    delete_path(&d).unwrap();
    assert!(!d.exists());

    fs::remove_dir_all(&root).ok();
}

#[test]
fn copy_file_into_dir() {
    let root = tmp_dir();
    let src = root.join("a.txt");
    fs::write(&src, b"hello").unwrap();
    let dst = root.join("dst");
    fs::create_dir_all(&dst).unwrap();

    copy_into(&src, &dst).unwrap();
    assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"hello");
    assert!(src.exists()); // копирование не удаляет источник

    fs::remove_dir_all(&root).ok();
}

#[test]
fn copy_dir_recursively() {
    let root = tmp_dir();
    let src = root.join("tree");
    fs::create_dir_all(src.join("a/b")).unwrap();
    fs::write(src.join("a/b/c.txt"), b"deep").unwrap();
    let dst = root.join("dst");
    fs::create_dir_all(&dst).unwrap();

    copy_into(&src, &dst).unwrap();
    assert_eq!(fs::read(dst.join("tree/a/b/c.txt")).unwrap(), b"deep");

    fs::remove_dir_all(&root).ok();
}

#[test]
fn move_file_into_dir() {
    let root = tmp_dir();
    let src = root.join("m.txt");
    fs::write(&src, b"z").unwrap();
    let dst = root.join("dst");
    fs::create_dir_all(&dst).unwrap();

    move_into(&src, &dst).unwrap();
    assert!(!src.exists());
    assert_eq!(fs::read(dst.join("m.txt")).unwrap(), b"z");

    fs::remove_dir_all(&root).ok();
}

#[test]
fn move_to_renames() {
    let root = tmp_dir();
    let src = root.join("a.txt");
    fs::write(&src, b"data").unwrap();
    let dst = root.join("b.txt");

    move_to(&src, &dst).unwrap();
    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), b"data");

    fs::remove_dir_all(&root).ok();
}

#[test]
fn copy_to_renames() {
    let root = tmp_dir();
    let src = root.join("a.txt");
    fs::write(&src, b"data").unwrap();
    let dst = root.join("copy.txt");

    copy_to(&src, &dst).unwrap();
    assert!(src.exists()); // копия — оригинал остаётся
    assert_eq!(fs::read(&dst).unwrap(), b"data");

    fs::remove_dir_all(&root).ok();
}
