//! Тесты автодополнения имён (Shift+Tab).

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

/// Временная директория с заданными именами.
fn tmp_with(names: &[&str]) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rfm_compl_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    for name in names {
        if name.ends_with('/') {
            std::fs::create_dir_all(dir.join(name.trim_end_matches('/'))).unwrap();
        } else {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
    }
    dir
}

fn buf(s: &str) -> CmdLine {
    CmdLine::from_str(s)
}

fn backtab() -> KeyEvent {
    KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)
}
fn ch(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

#[test]
fn unique_prefix_completes_immediately() {
    let dir = tmp_with(&["test", "other"]);
    let mut cmd = buf("te");
    let mut comp = None;
    // Shift+Tab: единственное совпадение "test" → сразу дополняем.
    let r = complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    assert!(matches!(r, CompleteKey::Consumed(None)));
    assert_eq!(cmd.text(), "test");
    assert!(comp.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ambiguous_enters_selection_mode_and_navigates() {
    let dir = tmp_with(&["test1", "test2", "test3"]);
    let mut cmd = buf("test");
    let mut comp = None;
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    // Несколько совпадений → режим выбора, строка пока не меняется.
    let c = comp.as_ref().expect("selection mode");
    assert_eq!(c.candidates, vec!["test1", "test2", "test3"]);
    assert_eq!(c.sel, 0);
    assert_eq!(cmd.text(), "test");

    // Shift+Tab / → двигают выбор, ← назад (по кругу).
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    assert_eq!(comp.as_ref().unwrap().sel, 1);
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(comp.as_ref().unwrap().sel, 0);

    // Enter — принять выбранный.
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(cmd.text(), "test1");
    assert!(comp.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn typing_in_mode_rebuilds_candidates() {
    let dir = tmp_with(&["alpha", "alpine", "beta"]);
    let mut cmd = buf("al");
    let mut comp = None;
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    assert_eq!(comp.as_ref().unwrap().candidates.len(), 2); // alpha, alpine
    // Печатаем 'p' → "alp", список перестраивается (обе ещё подходят).
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, ch('p'));
    assert_eq!(cmd.text(), "alp");
    assert_eq!(comp.as_ref().unwrap().candidates.len(), 2);
    // Печатаем 'h' → "alph" → только "alpha".
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, ch('h'));
    assert_eq!(comp.as_ref().unwrap().candidates, vec!["alpha"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn completes_path_component() {
    // /<tmp>/sub/  — дополняем внутри вложенной директории.
    let dir = tmp_with(&["sub/"]);
    std::fs::write(dir.join("sub").join("binfile"), b"x").unwrap();
    let typed = format!("{}/b", dir.join("sub").display());
    let mut cmd = buf(&typed);
    let mut comp = None;
    complete_key(&mut cmd, &mut comp, None, false, true, backtab());
    assert_eq!(cmd.text(), format!("{}/binfile", dir.join("sub").display()));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn directory_gets_trailing_slash() {
    let dir = tmp_with(&["mydir/"]);
    let mut cmd = buf("my");
    let mut comp = None;
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    assert_eq!(cmd.text(), "mydir/"); // директория → со слэшем
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn quotes_names_with_spaces_in_cmdline() {
    let dir = tmp_with(&["my file.txt"]);
    let mut cmd = buf("my");
    let mut comp = None;
    // quote=true (командная строка) → имя с пробелом в кавычках.
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    assert_eq!(cmd.text(), "'my file.txt'");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_quote_in_dialog_context() {
    let dir = tmp_with(&["my file.txt"]);
    let mut cmd = buf("my");
    let mut comp = None;
    // quote=false (диалог пути) → «сырое» имя без кавычек.
    complete_key(&mut cmd, &mut comp, Some(&dir), true, false, backtab());
    assert_eq!(cmd.text(), "my file.txt");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn completes_only_last_token_in_cmdline() {
    let dir = tmp_with(&["target"]);
    let mut cmd = buf("cp src ta");
    let mut comp = None;
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    assert_eq!(cmd.text(), "cp src target");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_match_reports_and_leaves_line() {
    let dir = tmp_with(&["test"]);
    let mut cmd = buf("zzz");
    let mut comp = None;
    let r = complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    match r {
        CompleteKey::Consumed(Some(msg)) => assert!(msg.contains("no match")),
        _ => panic!("expected no-match message"),
    }
    assert_eq!(cmd.text(), "zzz");
    assert!(comp.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn esc_cancels_selection() {
    let dir = tmp_with(&["test1", "test2"]);
    let mut cmd = buf("test");
    let mut comp = None;
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, backtab());
    assert!(comp.is_some());
    complete_key(&mut cmd, &mut comp, Some(&dir), false, true, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(comp.is_none());
    assert_eq!(cmd.text(), "test"); // строка не изменилась
    std::fs::remove_dir_all(&dir).ok();
}
