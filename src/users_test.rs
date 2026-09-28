//! Тесты резолва имён через libc. Опираемся на факты, истинные на любой unix:
//! uid/gid 0 существует (root/root|wheel), а заведомо огромный id — нет.

use super::*;

#[test]
fn root_resolves_to_name() {
    // uid 0 — всегда "root" на unix.
    assert_eq!(user_name(0), "root");
    // gid 0 — root (Linux) или wheel (BSD/macOS); в любом случае не число.
    let g = group_name(0);
    assert!(!g.is_empty());
    assert_ne!(g, "0", "gid 0 must resolve to a name");
}

#[test]
fn unknown_id_falls_back_to_number() {
    // Заведомо несуществующий id → само число.
    assert_eq!(user_name(4_000_000_000), "4000000000");
    assert_eq!(group_name(4_000_000_000), "4000000000");
}

#[test]
fn hostname_is_nonempty_and_short() {
    let h = hostname();
    assert!(!h.is_empty());
    assert!(!h.contains('.'), "short hostname must have no dots: {h}");
    assert!(!h.contains(char::is_whitespace));
}

#[test]
fn results_are_cached() {
    // Повторный вызов даёт тот же результат (проверяем кэш-путь).
    let a = user_name(0);
    let b = user_name(0);
    assert_eq!(a, b);
}
