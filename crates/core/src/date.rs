//! Calendar and display helpers.

use chrono::{Datelike, Local, Timelike};

use crate::{ClassBlock, WEEKDAYS};

/// Local clock reading plus the calendar fields the UI needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub unix_secs: u64,
    /// Minutes since midnight.
    pub minutes: u32,
    /// 0 = Monday .. 6 = Sunday.
    pub weekday: usize,
    /// Day of month.
    pub day: u32,
    /// 1..12.
    pub month: u32,
    pub year: i32,
}

pub fn now_local() -> LocalTime {
    let now = Local::now();
    LocalTime {
        unix_secs: now.timestamp().max(0) as u64,
        minutes: now.hour() * 60 + now.minute(),
        weekday: now.weekday().num_days_from_monday() as usize,
        day: now.day(),
        month: now.month(),
        year: now.year(),
    }
}

/// Nominative Russian month name, e.g. "Апрель 2026".
pub fn month_title_ru(month: u32, year: i32) -> String {
    const MONTHS: [&str; 12] = [
        "Январь", "Февраль", "Март", "Апрель", "Май", "Июнь", "Июль", "Август", "Сентябрь",
        "Октябрь", "Ноябрь", "Декабрь",
    ];
    let name = if (1..=12).contains(&month) {
        MONTHS[(month - 1) as usize]
    } else {
        MONTHS[0]
    };
    format!("{name} {year}")
}

pub fn month_length(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 30,
    }
}

/// Day-of-month numbers for the Monday-based week containing today.
pub fn week_dates(today_weekday: usize, today_dom: u32) -> [u32; WEEKDAYS] {
    // ponytail: wrap using the current month's length; near a boundary the adjacent month may differ by a day or two.
    let now = now_local();
    let len = month_length(now.year, now.month);
    let idx = today_weekday.min(WEEKDAYS - 1);
    let mut out = [0u32; WEEKDAYS];
    out[idx] = today_dom;
    for i in (0..idx).rev() {
        out[i] = if out[i + 1] <= 1 { len } else { out[i + 1] - 1 };
    }
    for i in idx + 1..WEEKDAYS {
        out[i] = if out[i - 1] == len { 1 } else { out[i - 1] + 1 };
    }
    out
}

/// Earliest block starting strictly after `now_min`.
pub fn next_class_start(blocks: &[ClassBlock], now_min: u32) -> Option<u32> {
    blocks.iter().map(|b| b.start_min).filter(|&m| m > now_min).min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockKind;

    fn block(start_min: u32) -> ClassBlock {
        ClassBlock {
            title: "Class".into(),
            room: "A-1".into(),
            start_min,
            end_min: start_min + 90,
            kind: BlockKind::Lecture,
        }
    }

    #[test]
    fn month_length_leap_years() {
        assert_eq!(month_length(2024, 2), 29);
        assert_eq!(month_length(2023, 2), 28);
        assert_eq!(month_length(2000, 2), 29);
        assert_eq!(month_length(1900, 2), 28);
        assert_eq!(month_length(2026, 1), 31);
        assert_eq!(month_length(2026, 4), 30);
    }

    #[test]
    fn week_dates_layout() {
        assert_eq!(week_dates(0, 10), [10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(week_dates(3, 10), [7, 8, 9, 10, 11, 12, 13]);
        assert_eq!(week_dates(6, 10), [4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn week_dates_wraps_month() {
        let week = week_dates(3, 1);
        assert_eq!(&week[3..], &[1, 2, 3, 4]);
        assert!(week[2] >= 28);
    }

    #[test]
    fn next_class_start_picks_earliest_future() {
        let blocks = [block(540), block(660)];
        assert_eq!(next_class_start(&blocks, 500), Some(540));
        assert_eq!(next_class_start(&blocks, 540), Some(660));
        assert_eq!(next_class_start(&blocks, 700), None);
    }

    #[test]
    fn local_time_fields_are_in_range() {
        let now = now_local();
        assert!(now.weekday < WEEKDAYS);
        assert!(now.minutes < 24 * 60);
        assert!((1..=12).contains(&now.month));
        assert_eq!(month_title_ru(4, 2026), "Апрель 2026");
    }
}
