//! Single-user local persistence: one JSON file, no accounts, no cloud.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{ScheduleOverrides, ScheduleState, WeatherInfo};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Settings {
    pub cached_weather: Option<WeatherInfo>,
    /// Unix seconds of the last successful weather fetch.
    pub last_weather_fetch: u64,
    /// Unix seconds of the upcoming alarms.
    pub next_alarms: Vec<u64>,
    pub streak_count: i32,
    pub notifications_enabled: bool,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct AppData {
    pub schedule_state: ScheduleState,
    pub overrides: ScheduleOverrides,
    pub settings: Settings,
}

/// Location of the single local data file.
pub fn data_path() -> PathBuf {
    // ponytail: hardcode the app's private files dir on Android; HOME is unreliable for NativeActivity.
    #[cfg(target_os = "android")]
    let dir = PathBuf::from("/data/data/com.frayed.app/files");
    #[cfg(not(target_os = "android"))]
    let dir = std::env::var("ANDROID_APP_DATA_DIR")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("frayed"));
    dir.join("frayed.json")
}

/// Missing or corrupt data is treated as a fresh install.
pub fn load() -> AppData {
    let Ok(bytes) = std::fs::read(data_path()) else {
        return AppData::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(data: &AppData) -> Result<(), String> {
    let path = data_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create data dir: {e}"))?;
    }
    let bytes = serde_json::to_vec(data).map_err(|e| format!("serialize data: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("write data: {e}"))?;
    // ponytail: Windows rename won't overwrite; remove first (brief window without a file).
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("replace data: {e}"))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename data: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("frayed-test-{}", std::process::id()));
        std::env::set_var("ANDROID_APP_DATA_DIR", &dir);

        let mut data = AppData::default();
        data.settings.streak_count = 7;
        data.settings.notifications_enabled = true;
        data.settings.last_weather_fetch = 1_700_000_000;
        data.settings.next_alarms = vec![1_700_003_600];
        data.settings.cached_weather = Some(WeatherInfo::sunny(21.0));
        save(&data).unwrap();

        let loaded = load();
        assert_eq!(loaded.settings.streak_count, 7);
        assert_eq!(loaded.settings.cached_weather, data.settings.cached_weather);
        assert_eq!(loaded.settings.next_alarms, data.settings.next_alarms);

        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("ANDROID_APP_DATA_DIR");
    }
}
