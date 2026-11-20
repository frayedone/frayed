//! `frayed-core`: домейн-логика личного расписания, погоды, транспорта и обновлений.

use serde::{Deserialize, Serialize};

pub mod date;
pub mod storage;

pub use date::{month_length, month_title_ru, next_class_start, now_local, week_dates, LocalTime};
pub use storage::{data_path, load, save, AppData, Settings};

pub const WEEKDAYS: usize = 7;

// ---------------------------------------------------------------------------
// Schedule model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockKind {
    Lecture,
    Lab,
    Test,
    Practice,
    Break,
    Custom,
}

impl BlockKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lecture => "lecture",
            Self::Lab => "lab",
            Self::Test => "test",
            Self::Practice => "practice",
            Self::Break => "break",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassBlock {
    pub title: String,
    pub room: String,
    pub start_min: u32,
    pub end_min: u32,
    pub kind: BlockKind,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DaySchedule {
    pub blocks: Vec<ClassBlock>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub days: Vec<DaySchedule>,
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            days: vec![DaySchedule::default(); WEEKDAYS],
        }
    }

    pub fn day_mut(&mut self, weekday: usize) -> &mut DaySchedule {
        &mut self.days[weekday]
    }

    pub fn day(&self, weekday: usize) -> DaySchedule {
        self.days.get(weekday).cloned().unwrap_or_default()
    }

    /// Effective day: override wins, cancelled days have no blocks.
    pub fn effective_day(&self, weekday: usize, overrides: &ScheduleOverrides) -> DaySchedule {
        if let Some(o) = overrides.days.iter().find(|o| o.weekday == weekday) {
            if o.cancelled {
                return DaySchedule::default();
            }
            return DaySchedule {
                blocks: o.blocks.clone(),
            };
        }
        self.days[weekday].clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPreset {
    pub id: String,
    pub name: String,
    pub schedule: Schedule,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScheduleState {
    pub presets: Vec<GroupPreset>,
    pub active_preset_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DayOverride {
    pub weekday: usize,
    pub cancelled: bool,
    pub blocks: Vec<ClassBlock>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScheduleOverrides {
    pub days: Vec<DayOverride>,
}

// ---------------------------------------------------------------------------
// Theme tokens (Dotify-inspired palette)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

pub const BG_PRIMARY: Rgba = Rgba { r: 0.043, g: 0.059, b: 0.078, a: 1.0 }; // #0B0F14
pub const SURFACE_CARD: Rgba = Rgba { r: 0.094, g: 0.118, b: 0.153, a: 1.0 }; // #181E29
pub const SURFACE_ELEVATED: Rgba = Rgba { r: 0.118, g: 0.153, b: 0.196, a: 1.0 }; // #1E2532
pub const TEXT_PRIMARY: Rgba = Rgba { r: 0.973, g: 0.980, b: 0.984, a: 1.0 }; // #F8FAFC
pub const TEXT_SECONDARY: Rgba = Rgba { r: 0.62, g: 0.66, b: 0.71, a: 1.0 };
pub const ACCENT_PRIMARY: Rgba = Rgba { r: 0.133, g: 0.839, b: 0.333, a: 1.0 }; // #22C55E
pub const ACCENT_SECONDARY: Rgba = Rgba { r: 0.22, g: 0.74, b: 0.98, a: 1.0 }; // #38BDF8
pub const BORDER_SUBTLE: Rgba = Rgba { r: 0.14, g: 0.18, b: 0.24, a: 1.0 };

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub bg_primary: Rgba,
    pub surface_card: Rgba,
    pub surface_elevated: Rgba,
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub accent_primary: Rgba,
    pub accent_secondary: Rgba,
    pub border_subtle: Rgba,
}

pub fn theme() -> Theme {
    Theme {
        bg_primary: BG_PRIMARY,
        surface_card: SURFACE_CARD,
        surface_elevated: SURFACE_ELEVATED,
        text_primary: TEXT_PRIMARY,
        text_secondary: TEXT_SECONDARY,
        accent_primary: ACCENT_PRIMARY,
        accent_secondary: ACCENT_SECONDARY,
        border_subtle: BORDER_SUBTLE,
    }
}

// ---------------------------------------------------------------------------
// Time helpers
// ---------------------------------------------------------------------------

pub fn time_to_minutes(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 || parts[0].len() != 2 || parts[1].len() != 2 {
        return None;
    }
    let hours: u32 = parts[0].parse().ok()?;
    let minutes: u32 = parts[1].parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(hours * 60 + minutes)
}

pub fn minutes_to_time(min: u32) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

pub fn weekday_name(weekday: usize) -> &'static str {
    const NAMES: [&str; WEEKDAYS] = [
        "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
    ];
    NAMES.get(weekday).copied().unwrap_or("Monday")
}

pub fn weekday_letter(weekday: usize) -> char {
    const LETTERS: [char; WEEKDAYS] = ['M', 'T', 'W', 'T', 'F', 'S', 'S'];
    LETTERS.get(weekday).copied().unwrap_or('M')
}

/// Blocks of the effective day, ordered by start time.
pub fn effective_blocks(
    schedule: &Schedule,
    overrides: &ScheduleOverrides,
    weekday: usize,
) -> Vec<ClassBlock> {
    let day = schedule.effective_day(weekday, overrides);
    let mut blocks: Vec<ClassBlock> = day.blocks;
    blocks.sort_by_key(|b| b.start_min);
    blocks
}

pub fn day_is_cancelled(_schedule: &Schedule, overrides: &ScheduleOverrides, weekday: usize) -> bool {
    overrides
        .days
        .iter()
        .find(|o| o.weekday == weekday)
        .map(|o| o.cancelled)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Weather / departure hints
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeatherInfo {
    pub temperature_c: f32,
    pub apparent_temperature_c: f32,
    pub is_rain: bool,
    pub is_snow: bool,
}

/// Precipitation flags from a WMO weather code returned by Open-Meteo.
pub fn weather_flags_from_code(code: i32) -> (bool, bool) {
    let rain = (51..=67).contains(&code) || (80..=82).contains(&code);
    let snow = (71..=77).contains(&code) || (85..=86).contains(&code);
    (rain, snow)
}

impl WeatherInfo {
    pub fn sunny(temperature_c: f32) -> Self {
        Self {
            temperature_c,
            apparent_temperature_c: temperature_c,
            is_rain: false,
            is_snow: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DepartureHint {
    pub leave_now: bool,
    pub reason: String,
    pub minutes_before_class: u32,
}

pub fn compute_departure_hint(
    weather: &WeatherInfo,
    next_class_start_min: Option<u32>,
    now_min: u32,
) -> DepartureHint {
    let bad_weather = weather.is_rain || weather.is_snow;
    let cold = weather.temperature_c < 5.0;
    let minutes_to_class = next_class_start_min.map(|s| s.saturating_sub(now_min));
    let leave_now = bad_weather || cold || minutes_to_class.is_some_and(|m| m <= 45);

    DepartureHint {
        leave_now,
        reason: if bad_weather {
            "bad weather".into()
        } else if cold {
            "cold outside".into()
        } else if minutes_to_class.is_some_and(|m| m <= 45) {
            "class soon".into()
        } else {
            String::new()
        },
        minutes_before_class: minutes_to_class.unwrap_or(0),
    }
}

// ---------------------------------------------------------------------------
// Semantic version (GitHub releases)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl SemVer {
    pub fn parse(input: &str) -> Option<Self> {
        let s = input.trim().trim_start_matches('v').trim();
        let mut parts = s.splitn(3, '.');
        let major: u64 = parts.next()?.parse().ok()?;
        let minor: u64 = parts.next()?.parse().ok()?;
        let patch: u64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Some(Self { major, minor, patch })
    }
}

/// True when `latest` is a higher version than `current`.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (SemVer::parse(current), SemVer::parse(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Theme helpers
// ---------------------------------------------------------------------------

impl Theme {
    pub fn to_rgba(r: u8, g: u8, b: u8) -> Rgba {
        Rgba {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_roundtrip() {
        assert_eq!(time_to_minutes("09:30"), Some(570));
        assert_eq!(time_to_minutes("23:59"), Some(1439));
        assert_eq!(time_to_minutes("9:7"), None);
        assert_eq!(minutes_to_time(570), "09:30");
        assert_eq!(minutes_to_time(0), "00:00");
    }

    #[test]
    fn semver_compare() {
        assert_eq!(SemVer::parse("v1.2.3"), Some(SemVer { major: 1, minor: 2, patch: 3 }));
        assert_eq!(SemVer::parse("1.10.0"), Some(SemVer { major: 1, minor: 10, patch: 0 }));
        assert!(SemVer::parse("v1.2.4").unwrap() > SemVer::parse("v1.2.3").unwrap());
        assert!(SemVer::parse("v0.9.9").unwrap() < SemVer::parse("v1.0.0").unwrap());
        assert_eq!(SemVer::parse("junk"), None);
    }

    #[test]
    fn is_newer_versions() {
        assert!(is_newer("v1.2.3", "v1.3.0"));
        assert!(is_newer("1.2", "1.2.1"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("2.0.0", "1.0.0"));
        assert!(!is_newer("junk", "1.0.0"));
    }

    #[test]
    fn weather_flags_from_wmo_code() {
        assert_eq!(weather_flags_from_code(0), (false, false));
        assert_eq!(weather_flags_from_code(51), (true, false));
        assert_eq!(weather_flags_from_code(67), (true, false));
        assert_eq!(weather_flags_from_code(71), (false, true));
        assert_eq!(weather_flags_from_code(77), (false, true));
        assert_eq!(weather_flags_from_code(80), (true, false));
        assert_eq!(weather_flags_from_code(82), (true, false));
        assert_eq!(weather_flags_from_code(85), (false, true));
        assert_eq!(weather_flags_from_code(86), (false, true));
        assert_eq!(weather_flags_from_code(95), (false, false));
    }

    #[test]
    fn cancelled_day_has_no_blocks() {
        let mut schedule = Schedule::new();
        schedule.day_mut(2).blocks.push(ClassBlock {
            title: "Math".into(),
            room: "A-1".into(),
            start_min: 540,
            end_min: 630,
            kind: BlockKind::Lecture,
        });
        let mut overrides = ScheduleOverrides::default();
        overrides.days.push(DayOverride { weekday: 2, cancelled: true, blocks: vec![] });
        assert!(day_is_cancelled(&schedule, &overrides, 2));
        assert!(effective_blocks(&schedule, &overrides, 2).is_empty());
    }

    #[test]
    fn override_replaces_blocks() {
        let mut schedule = Schedule::new();
        schedule.day_mut(3).blocks.push(ClassBlock {
            title: "Physics".into(),
            room: "B-204".into(),
            start_min: 660,
            end_min: 780,
            kind: BlockKind::Lab,
        });
        let mut overrides = ScheduleOverrides::default();
        overrides.days.push(DayOverride {
            weekday: 3,
            cancelled: false,
            blocks: vec![ClassBlock {
                title: "Physics".into(),
                room: "B-204".into(),
                start_min: 660,
                end_min: 780,
                kind: BlockKind::Lab,
            }],
        });
        let blocks = effective_blocks(&schedule, &overrides, 3);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].title, "Physics");
    }

    #[test]
    fn departure_hint_bad_weather() {
        let weather = WeatherInfo {
            temperature_c: 24.0,
            apparent_temperature_c: 22.0,
            is_rain: true,
            is_snow: false,
        };
        let hint = compute_departure_hint(&weather, Some(540), 300);
        assert!(hint.leave_now);
        assert!(!hint.reason.is_empty());
    }
}
