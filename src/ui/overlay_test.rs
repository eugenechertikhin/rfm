use super::history_scroll_top;

#[test]
fn scroll_stays_while_sel_visible() {
    assert_eq!(history_scroll_top(0, 0, 12, 30), 0);
    assert_eq!(history_scroll_top(0, 11, 12, 30), 0);
    assert_eq!(history_scroll_top(5, 10, 12, 30), 5);
}

#[test]
fn scroll_down_by_one_past_bottom_edge() {
    assert_eq!(history_scroll_top(0, 12, 12, 30), 1);
    assert_eq!(history_scroll_top(1, 13, 12, 30), 2);
    assert_eq!(history_scroll_top(0, 29, 12, 30), 18); // End: последняя строка внизу окна
}

#[test]
fn scroll_up_past_top_edge() {
    assert_eq!(history_scroll_top(5, 4, 12, 30), 4);
    assert_eq!(history_scroll_top(18, 0, 12, 30), 0); // Home
}

#[test]
fn scroll_clamped_to_list_end() {
    // Список сократился — без пустоты внизу окна.
    assert_eq!(history_scroll_top(18, 2, 12, 5), 0);
    assert_eq!(history_scroll_top(18, 20, 12, 25), 13);
}

#[test]
fn scroll_degenerate_cases() {
    assert_eq!(history_scroll_top(3, 0, 12, 0), 0); // пустой список
    assert_eq!(history_scroll_top(3, 5, 0, 30), 0); // окно без строк
    assert_eq!(history_scroll_top(0, 4, 2, 30), 3); // низкое окно
}
