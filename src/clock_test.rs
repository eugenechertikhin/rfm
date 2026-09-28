//! Тест формата локального времени `hh:mm`.

use super::*;

#[test]
fn hh_mm_format() {
    let s = hh_mm();
    assert_eq!(s.len(), 5, "expected HH:MM, got {s:?}");
    let b = s.as_bytes();
    assert_eq!(b[2], b':');
    assert!(b[0].is_ascii_digit() && b[1].is_ascii_digit());
    assert!(b[3].is_ascii_digit() && b[4].is_ascii_digit());
    // Часы 00..23, минуты 00..59.
    let hh: u32 = s[0..2].parse().unwrap();
    let mm: u32 = s[3..5].parse().unwrap();
    assert!(hh < 24 && mm < 60);
}
