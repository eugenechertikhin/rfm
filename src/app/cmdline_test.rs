use super::*;

#[test]
fn delete_removes_char_under_cursor() {
    let mut c = CmdLine::from_str("abcd");
    c.home();
    c.right(); // курсор на 'b'
    c.delete();
    assert_eq!(c.text(), "acd");
    assert_eq!(c.cursor, 1);
}

#[test]
fn delete_at_end_is_noop() {
    let mut c = CmdLine::from_str("abc");
    c.delete();
    assert_eq!(c.text(), "abc");
    assert_eq!(c.cursor, 3);
}

#[test]
fn delete_on_empty_is_noop() {
    let mut c = CmdLine::default();
    c.delete();
    assert_eq!(c.text(), "");
    assert_eq!(c.cursor, 0);
}
