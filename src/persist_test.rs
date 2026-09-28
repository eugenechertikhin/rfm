//! Юнит-тесты модуля `persist` (в отдельном файле — по уставу).

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn tmp_file() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("rfm_panel_{}_{}", std::process::id(), n))
}

#[test]
fn roundtrip_dirs_viewer_and_editor() {
    let file = tmp_file();
    let d1 = std::env::temp_dir();
    // существующие файлы для viewer/editor
    let vfile = std::env::temp_dir().join(format!("rfm_pv_{}", std::process::id()));
    fs::write(&vfile, b"x").unwrap();
    let efile = std::env::temp_dir().join(format!("rfm_pe_{}", std::process::id()));
    fs::write(&efile, b"y").unwrap();

    let panels = vec![
        PanelState {
            dir: d1.clone(),
            columns: 3,
            view_override: Some(FileListView::Tree),
            viewer: Some((vfile.clone(), 7)),
            editor: None,
        },
        PanelState {
            dir: d1.clone(),
            columns: 1,
            view_override: None,
            viewer: None,
            editor: Some((efile.clone(), 12, 4)),
        },
    ];
    save_to(&file, 1, &panels).unwrap();
    let (active, got) = load_from(&file).unwrap();
    assert_eq!(active, 1);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].columns, 3);
    assert_eq!(got[0].view_override, Some(FileListView::Tree));
    assert_eq!(got[0].viewer, Some((vfile.clone(), 7)));
    assert_eq!(got[0].editor, None);
    assert_eq!(got[1].viewer, None);
    assert_eq!(got[1].columns, 1);
    assert_eq!(got[1].editor, Some((efile.clone(), 12, 4)));

    fs::remove_file(&file).ok();
    fs::remove_file(&vfile).ok();
    fs::remove_file(&efile).ok();
}

#[test]
fn editor_with_missing_file_dropped() {
    let file = tmp_file();
    let good = std::env::temp_dir();
    let gone = std::env::temp_dir().join("rfm_editor_missing_zzz");
    fs::write(
        &file,
        format!("0\n{}\t1\t\t\t\t{}\t3\t4\n", good.display(), gone.display()),
    )
    .unwrap();
    let (_, got) = load_from(&file).unwrap();
    assert_eq!(got[0].editor, None);
    fs::remove_file(&file).ok();
}

#[test]
fn missing_dirs_filtered() {
    let file = tmp_file();
    let good = std::env::temp_dir();
    let bad = std::env::temp_dir().join("rfm_missing_zzz");
    fs::write(&file, format!("5\n{}\n{}\n", good.display(), bad.display())).unwrap();
    let (active, got) = load_from(&file).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(active, 0);
    fs::remove_file(&file).ok();
}
