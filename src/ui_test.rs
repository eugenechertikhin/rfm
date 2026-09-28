//! Юнит-тесты модуля `ui` (в отдельном файле — по уставу).

use super::panel::{file_info_line, format_entry, format_mode, human_size};
use super::*;

fn mk(name: &str, kind: crate::vfs::EntryKind, mode: Option<u32>) -> VfsEntry {
    VfsEntry {
        name: name.to_string(),
        kind,
        size: Some(1024),
        permissions: mode,
        owner: Some("root".to_string()),
        group: Some("wheel".to_string()),
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: false,
        depth: 0,
    }
}

#[test]
fn mode_basic_and_type() {
    use crate::vfs::EntryKind::*;
    assert_eq!(format_mode(&mk("f", File, Some(0o644)), Some(0o644)), "-rw-r--r--");
    assert_eq!(format_mode(&mk("d", Dir, Some(0o755)), Some(0o755)), "drwxr-xr-x");
    assert_eq!(format_mode(&mk("l", Symlink, Some(0o777)), Some(0o777)), "lrwxrwxrwx");
    // Без прав — прочерки, но тип сохраняется.
    assert_eq!(format_mode(&mk("f", File, None), None), "----------");
}

#[test]
fn mode_special_bits() {
    use crate::vfs::EntryKind::File;
    // setuid с x → 's', без x → 'S'.
    assert_eq!(format_mode(&mk("f", File, Some(0o4755)), Some(0o4755)), "-rwsr-xr-x");
    assert_eq!(format_mode(&mk("f", File, Some(0o4655)), Some(0o4655)), "-rwSr-xr-x");
    // setgid.
    assert_eq!(format_mode(&mk("f", File, Some(0o2755)), Some(0o2755)), "-rwxr-sr-x");
    // sticky в триаде остальных → 't'/'T'.
    assert_eq!(format_mode(&mk("d", crate::vfs::EntryKind::Dir, Some(0o1777)), Some(0o1777)), "drwxrwxrwt");
    assert_eq!(format_mode(&mk("d", crate::vfs::EntryKind::Dir, Some(0o1666)), Some(0o1666)), "drw-rw-rwT");
}

#[test]
fn info_line_contains_all_fields() {
    let e = mk("file.txt", crate::vfs::EntryKind::File, Some(0o644));
    let line = file_info_line(&e, 60);
    assert_eq!(UnicodeWidthStr::width(line.as_str()), 60);
    assert!(line.contains("file.txt"));
    assert!(line.contains("1.0K"));
    assert!(line.contains("root:wheel"));
    assert!(line.contains("-rw-r--r--"));
}

#[test]
fn truncate_respects_width() {
    assert_eq!(truncate_width("hello", 3), "hel");
    assert_eq!(truncate_width("hello", 10), "hello");
}

#[test]
fn truncate_wide_chars() {
    // Каждый CJK-символ шириной 2.
    assert_eq!(truncate_width("日本語", 4), "日本");
    assert_eq!(truncate_width("日本語", 5), "日本");
}

#[test]
fn human_size_units() {
    assert_eq!(human_size(512), "512B");
    assert_eq!(human_size(1024), "1.0K");
    assert_eq!(human_size(1536), "1.5K");
}

#[test]
fn format_entry_fits_width() {
    let e = VfsEntry {
        name: "file.txt".to_string(),
        kind: crate::vfs::EntryKind::File,
        size: Some(2048),
        permissions: None,
        owner: None,
        group: None,
        mtime: None,
        executable: false,
        symlink_target: None,
        target_is_dir: false,
        depth: 0,
    };
    let line = format_entry(&e, 20);
    assert_eq!(UnicodeWidthStr::width(line.as_str()), 20);
    assert!(line.starts_with("file.txt"));
    assert!(line.trim_end().ends_with("2.0K"));
}
