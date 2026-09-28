//! Юнит-тесты модуля `history` (в отдельном файле — по уставу).

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn tmp_file() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("rfm_hist_{}_{}", std::process::id(), n))
}

#[test]
fn record_increments_count() {
    let mut h = History::default();
    h.record_at("ls", 100);
    h.record_at("ls", 200);
    h.record_at("pwd", 150);
    let ls = h.entries.iter().find(|e| e.cmd == "ls").unwrap();
    assert_eq!(ls.count, 2);
    assert_eq!(ls.last, 200);
    assert_eq!(h.entries.len(), 2);
}

#[test]
fn recent_orders_by_last() {
    let mut h = History::default();
    h.record_at("a", 100);
    h.record_at("b", 300);
    h.record_at("c", 200);
    assert_eq!(h.recent(), vec!["b", "c", "a"]);
}

#[test]
fn ranked_filters_and_sorts_by_count() {
    let mut h = History::default();
    h.record_at("git status", 100);
    h.record_at("git status", 110); // count 2
    h.record_at("git commit", 120); // count 1
    h.record_at("ls", 130);
    let r = h.ranked("git");
    assert_eq!(r, vec!["git status", "git commit"]);
    assert!(h.ranked("").contains(&"ls".to_string()));
}

#[test]
fn empty_command_ignored() {
    let mut h = History::default();
    h.record_at("   ", 100);
    assert!(h.entries.is_empty());
}

#[test]
fn save_load_roundtrip() {
    let file = tmp_file();
    let entries = vec![
        HistEntry {
            cmd: "ls -la".to_string(),
            count: 3,
            last: 999,
        },
        HistEntry {
            cmd: "cd /tmp".to_string(),
            count: 1,
            last: 1000,
        },
    ];
    save_to(&file, &entries).unwrap();
    let loaded = load_from(&file).unwrap();
    assert_eq!(loaded, entries);
    fs::remove_file(&file).ok();
}
