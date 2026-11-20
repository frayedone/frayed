//! `frayed-net`: внешние клиенты — Open-Meteo, GitHub Releases, транспорт.

use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;

pub const GITHUB_OWNER: &str = "frayedone";
pub const GITHUB_REPO: &str = "frayed";
pub const OPEN_METEO_URL: &str = "https://api.open-meteo.com/v1/forecast";
pub const ALMATY_LAT: f64 = 43.2381;
pub const ALMATY_LON: f64 = 76.8897;

pub use frayed_core::{DepartureHint, SemVer, WeatherInfo};

#[derive(Debug, Clone)]
pub struct Release {
    pub tag_name: String,
    pub name: String,
    pub html_url: String,
    pub apk_url: String,
    pub version: String,
    pub prerelease: bool,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    current: Option<OpenMeteoCurrent>,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoCurrent {
    temperature_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    weather_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct OpenMeteoClient {
    pub client: Client,
}

impl OpenMeteoClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("http client"),
        }
    }
}

impl Default for OpenMeteoClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenMeteoClient {
    pub async fn fetch_weather(&self, latitude: f64, longitude: f64) -> Result<WeatherInfo, String> {
        let url = format!(
            "{OPEN_METEO_URL}?latitude={latitude}&longitude={longitude}&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m&daily=weather_code&forecast_days=7"
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("open-meteo request failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("open-meteo http error: {e}"))?;
        let data: OpenMeteoResponse = response
            .json()
            .await
            .map_err(|e| format!("open-meteo json error: {e}"))?;
        let current = data.current.ok_or_else(|| "open-meteo: missing current".to_string())?;
        let (is_rain, is_snow) = current
            .weather_code
            .map(frayed_core::weather_flags_from_code)
            .unwrap_or((false, false));
        Ok(WeatherInfo {
            temperature_c: current.temperature_2m.unwrap_or(0.0) as f32,
            apparent_temperature_c: current.apparent_temperature.unwrap_or(0.0) as f32,
            is_rain,
            is_snow,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GitHubReleaseClient {
    pub client: Client,
}

impl GitHubReleaseClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("http client"),
        }
    }
}

impl Default for GitHubReleaseClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubReleaseClient {
    pub async fn latest_release(&self) -> Result<Release, String> {
        let url = format!("https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/latest");
        let response = self
            .client
            .get(&url)
            .header("User-Agent", "frayed")
            .send()
            .await
            .map_err(|e| format!("github releases request failed: {e}"))?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err("github: no releases found".into());
        }
        if !status.is_success() {
            return Err(format!("github: http {status}"));
        }
        let release: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("github releases json error: {e}"))?;
        let tag_name = release["tag_name"].as_str().unwrap_or_default().to_string();
        let version = tag_name.trim_start_matches('v').to_string();
        let html_url = release["html_url"].as_str().unwrap_or_default().to_string();
        let release_name = release["name"].as_str().unwrap_or(&tag_name).to_string();
        let apk_url = format!(
            "https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}/releases/download/{tag_name}/frayed-{version}.apk"
        );
        Ok(Release {
            tag_name,
            name: release_name,
            html_url,
            apk_url,
            version,
            prerelease: release["prerelease"].as_bool().unwrap_or(false),
        })
    }
}

#[derive(Debug, Clone)]
pub struct TransitClient;

// (stop_id, route, first departure in minutes past midnight, headway in minutes)
// ponytail: static schedule, replace when a real GTFS feed is available
const STOPS: &[(&str, &str, u32, u32)] = &[
    ("kaznu", "12", 6 * 60 + 10, 12),
    ("abay", "29", 6 * 60 + 25, 15),
    ("tole-bi", "5", 6 * 60 + 40, 20),
    ("vuz", "95", 7 * 60 + 5, 10),
];

#[derive(Debug, Clone)]
pub struct TransitArrival {
    pub stop_id: String,
    pub route: String,
    pub minutes: i32,
}

fn static_departures() -> Vec<TransitArrival> {
    let now = frayed_core::now_local().minutes;
    STOPS.iter()
        .map(|&(stop, route, first, headway)| {
            let since_first = now as i32 - first as i32;
            let minutes = if since_first < 0 {
                -since_first
            } else {
                let wait = headway as i32 - since_first % headway as i32;
                if wait == 0 {
                    headway as i32
                } else {
                    wait
                }
            };
            TransitArrival {
                stop_id: stop.into(),
                route: route.into(),
                minutes,
            }
        })
        .collect()
}

impl TransitClient {
    pub fn new() -> Self {
        Self
    }

    pub fn arrivals(&self) -> Result<Vec<TransitArrival>, String> {
        Ok(static_departures())
    }
}

impl Default for TransitClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Static departures near the college area; `from`/`to` are accepted for callers but no routing is performed.
pub fn fetch_departures(_from: &str, _to: &str) -> Vec<TransitArrival> {
    static_departures()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_departures_use_time_of_day() {
        let arrivals = fetch_departures("kaznu", "vuz");
        assert_eq!(arrivals.len(), STOPS.len());
        assert!(arrivals.iter().all(|a| a.minutes > 0 && a.minutes <= 24 * 60));
        assert_eq!(arrivals[0].stop_id, "kaznu");
        assert_eq!(TransitClient::new().arrivals().unwrap().len(), STOPS.len());
    }
}

impl GitHubReleaseClient {
    pub async fn download_release_apk(&self, apk_url: &str, dest_path: &str) -> Result<(), String> {
        let response = self
            .client
            .get(apk_url)
            .header("User-Agent", "frayed")
            .send()
            .await
            .map_err(|e| format!("github apk download failed: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("github: apk http {status}"));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("github apk body failed: {e}"))?;
        std::fs::write(dest_path, &bytes).map_err(|e| format!("apk write failed: {e}"))?;
        Ok(())
    }
}
