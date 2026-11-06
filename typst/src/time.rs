//! Today's local date, without a time-zone database: the offset comes from
//! `date +%z` once per run.

use std::time::{SystemTime, UNIX_EPOCH};

const DAY: i64 = 86_400;

/// Wall-clock unix seconds.
pub fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) }

/// Civil time (year, month, day, hour, minute) of a unix timestamp, shifted by `offset` seconds.
pub fn civil_at(secs: i64, offset: i64) -> (i64, u32, u32, u32, u32) {
    let t = secs + offset;
    let days = t.div_euclid(DAY);
    let rem = t.rem_euclid(DAY);
    // Howard Hinnant's days-from-civil inverse.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32)
}

/// Local civil time from a unix timestamp.
pub fn civil(secs: u64) -> (i64, u32, u32, u32, u32) { civil_at(secs as i64, local_offset()) }

pub fn local_offset() -> i64 {
    use std::sync::OnceLock;
    static OFF: OnceLock<i64> = OnceLock::new();
    *OFF.get_or_init(|| {
        // `date +%z` prints e.g. +0800.
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| {
                let s = s.trim();
                let sign = if s.starts_with('-') { -1 } else { 1 };
                let digits = s.trim_start_matches(['+', '-']);
                let h: i64 = digits.get(0..2)?.parse().ok()?;
                let m: i64 = digits.get(2..4)?.parse().ok()?;
                Some(sign * (h * 3600 + m * 60))
            })
            .unwrap_or(0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_cross_years_and_leap_days() {
        assert_eq!(civil_at(0, 0), (1970, 1, 1, 0, 0));
        assert_eq!(civil_at(951_782_400, 0), (2000, 2, 29, 0, 0));
        assert_eq!(civil_at(1_704_067_199, 0), (2023, 12, 31, 23, 59));
        assert_eq!(civil_at(1_704_067_199, 3600), (2024, 1, 1, 0, 59));
    }
}
