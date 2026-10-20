//! Unix time and calendar parts without a date library.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn system_time_to_unix(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

pub fn now() -> i64 {
    system_time_to_unix(SystemTime::now())
}

/// `(year, month, day, hour, minute, second)` in UTC.
pub fn parts_from_unix(t: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400) as u32;
    // civil-from-days (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d, secs / 3600, secs % 3600 / 60, secs % 60)
}

pub fn date_text(t: i64) -> String {
    let (y, m, d, ..) = parts_from_unix(t);
    format!("{d:02}.{m:02}.{y}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_parts() {
        assert_eq!(parts_from_unix(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(parts_from_unix(1_700_000_000), (2023, 11, 14, 22, 13, 20));
        assert_eq!(parts_from_unix(951_782_400), (2000, 2, 29, 0, 0, 0));
        assert_eq!(date_text(1_700_000_000), "14.11.2023");
    }
}
