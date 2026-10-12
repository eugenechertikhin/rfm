//! Тесты операций поверх VFS: пути `Loc`, локальные операции, извлечение из архива,
//! ошибки «только чтение». Сетевые бэкенды проверяются на уровне сборки команд
//! (`sftp_test`, `ftp_test`).

use super::*;
use std::fs;

fn tmp(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let p = std::env::temp_dir().join(format!(
        "rfm_xfer_{tag}_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn sconn() -> SftpConn {
    SftpConn {
        target: sftp::SftpTarget { user: Some("u".into()), host: "h".into(), port: None, path: "/".into() },
        sock: PathBuf::from("/s"),
    }
}

fn arc(file: &Path, kind: &str, member: &str, is_dir: bool) -> Item {
    Item {
        loc: Loc::Archive(ArchiveSrc { file: file.to_path_buf(), kind: kind.into(), remote: None }, member.into()),
        is_dir,
    }
}

#[test]
fn loc_join_and_name() {
    let s = Loc::Sftp(sconn(), "/".into());
    assert_eq!(s.join("a"), Loc::Sftp(sconn(), "/a".into()));
    assert_eq!(s.join("a").join("b c").name(), "b c");
    assert_eq!(Loc::Sftp(sconn(), "/x".into()).join("/abs/y"), Loc::Sftp(sconn(), "/abs/y".into()));
    let a = arc(Path::new("/a.zip"), "zip", "", true).loc;
    assert_eq!(a.join("d").join("f").name(), "f");
    assert!(matches!(a.join("d").join("f"), Loc::Archive(_, ref p) if p == "d/f"));
    assert_eq!(Loc::Local(PathBuf::from("/x")).join("y"), Loc::Local(PathBuf::from("/x/y")));
    assert_eq!(s.join("d").display(), "sftp://u@h/d");
}

#[test]
fn same_server_ignores_path() {
    let a = sconn().target;
    let mut b = a.clone();
    b.path = "/other".into();
    assert!(same_sftp(&a, &b));
    b.port = Some(2222);
    assert!(!same_sftp(&a, &b));
}

#[test]
fn local_copy_move_mkdir_delete() {
    let d = tmp("local");
    fs::write(d.join("f"), b"1").unwrap();
    mkdir(&Loc::Local(d.join("sub/deep"))).unwrap();
    assert!(is_dir(&Loc::Local(d.join("sub/deep"))));
    copy(&Item::local(d.join("f")), &Loc::Local(d.join("sub/g"))).unwrap();
    assert_eq!(fs::read(d.join("sub/g")).unwrap(), b"1");
    move_(&Item::local(d.join("sub")), &Loc::Local(d.join("moved"))).unwrap();
    assert!(!d.join("sub").exists() && d.join("moved/g").exists());
    delete(&Item::local(d.join("moved"))).unwrap();
    assert!(!d.join("moved").exists());
    fs::remove_dir_all(&d).ok();
}

#[test]
fn archive_is_read_only() {
    let item = arc(Path::new("/a.zip"), "zip", "f", false);
    let dest = item.loc.clone();
    let e = |r: io::Result<()>| r.unwrap_err().to_string();
    assert_eq!(e(mkdir(&dest)), READ_ONLY);
    assert_eq!(e(delete(&item)), READ_ONLY);
    assert_eq!(e(move_(&item, &Loc::Local(PathBuf::from("/tmp/x")))), READ_ONLY);
    assert_eq!(e(copy(&Item::local(PathBuf::from("/etc/hosts")), &dest)), READ_ONLY);
    assert!(!is_dir(&dest));
}

#[test]
fn copy_from_tar_archive_to_local() {
    let d = tmp("arc");
    fs::create_dir_all(d.join("src/dir/sub")).unwrap();
    fs::write(d.join("src/dir/a.txt"), b"A").unwrap();
    let tar = d.join("x.tar");
    assert!(std::process::Command::new("tar")
        .arg("-cf").arg(&tar).arg("-C").arg(d.join("src")).arg("dir")
        .status().unwrap().success());
    // файл — в точный путь (с переименованием)
    copy(&arc(&tar, "tar", "dir/a.txt", false), &Loc::Local(d.join("renamed.txt"))).unwrap();
    assert_eq!(fs::read(d.join("renamed.txt")).unwrap(), b"A");
    // директория — целиком
    copy(&arc(&tar, "tar", "dir", true), &Loc::Local(d.join("out"))).unwrap();
    assert!(d.join("out/sub").is_dir() && d.join("out/a.txt").is_file());
    // неизвестный тип архива
    let err = copy(&arc(&tar, "cpio", "dir", true), &Loc::Local(d.join("o2"))).unwrap_err();
    assert_eq!(err.to_string(), "extract is not supported for cpio");
    fs::remove_dir_all(&d).ok();
}

#[test]
fn temp_dir_removed_on_drop() {
    let p = {
        let t = TempDir::new("test").unwrap();
        fs::write(t.path().join("x"), b"x").unwrap();
        t.path().to_path_buf()
    };
    assert!(!p.exists());
}

#[test]
fn local_copy_fetch_upload_and_cleanup() {
    let d = tmp("lcopy");
    let orig = d.join("f.json");
    fs::write(&orig, b"{}").unwrap();
    let c = LocalCopy::fetch(Item::local(orig.clone()), "view").unwrap();
    // имя (и расширение) сохраняется — нужно prettify
    assert_eq!(c.path.file_name().unwrap(), "f.json");
    assert_eq!(fs::read(&c.path).unwrap(), b"{}");
    let before = c.stamp();
    fs::write(&c.path, b"{\"a\":1}").unwrap();
    assert_ne!(c.stamp(), before);
    c.upload().unwrap();
    assert_eq!(fs::read(&orig).unwrap(), b"{\"a\":1}");
    let tmp_path = c.path.clone();
    drop(c);
    assert!(!tmp_path.exists());
    fs::remove_dir_all(&d).ok();
}

#[test]
fn local_copy_keep_leaves_file() {
    let d = tmp("keep");
    fs::write(d.join("k.txt"), b"k").unwrap();
    let c = LocalCopy::fetch(Item::local(d.join("k.txt")), "edit").unwrap();
    let kept = c.keep();
    assert_eq!(fs::read(&kept).unwrap(), b"k");
    fs::remove_dir_all(kept.parent().unwrap()).ok();
    fs::remove_dir_all(&d).ok();
}

#[test]
fn local_copy_upload_to_archive_is_read_only() {
    let d = tmp("ro");
    fs::write(d.join("a"), b"a").unwrap();
    let mut c = LocalCopy::fetch(Item::local(d.join("a")), "edit").unwrap();
    c.origin = arc(&d.join("x.tar"), "tar", "a", false);
    assert_eq!(c.upload().unwrap_err().to_string(), READ_ONLY);
    fs::remove_dir_all(&d).ok();
}

#[test]
fn remote_archive_unknown_kind_fails_before_ssh() {
    let item = Item {
        loc: Loc::Archive(
            ArchiveSrc { file: PathBuf::from("/srv/x.cpio"), kind: "cpio".into(), remote: Some(sconn()) },
            "a".into(),
        ),
        is_dir: false,
    };
    let err = fetch(&item, Path::new("/nonexistent/out")).unwrap_err();
    assert_eq!(err.to_string(), "extract is not supported for cpio");
}
