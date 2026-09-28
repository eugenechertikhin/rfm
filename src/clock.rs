//! Локальное время `hh:mm` через libc (`localtime_r`) — без крейтов, с учётом TZ.

/// Текущее локальное время в формате `hh:mm`.
pub fn hh_mm() -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        // localtime_r учитывает часовой пояс системы/переменную TZ.
        if libc::localtime_r(&t, &mut tm).is_null() {
            return String::new();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

#[cfg(test)]
#[path = "clock_test.rs"]
mod tests;
