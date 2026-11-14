use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::system::calendar::fetch_url_native;

pub const WM_APP_WEATHER_UPDATED: u32 = 0x8010;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherCondition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Thunderstorm,
    Snow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeatherInfo {
    pub temperature: f32,
    pub apparent_temperature: f32,
    pub weather_code: u32,
    pub is_day: bool,
    pub relative_humidity: u32,
    pub wind_speed: f32,
    pub temp_max: f32,
    pub temp_min: f32,
    pub city: String,
    pub is_offline: bool,
    pub is_loaded: bool,
    pub last_updated: Instant,
}

impl WeatherInfo {
    pub fn condition(&self) -> WeatherCondition {
        match self.weather_code {
            0 => WeatherCondition::Clear,
            1 | 2 => WeatherCondition::PartlyCloudy,
            3 => WeatherCondition::Cloudy,
            45 | 48 => WeatherCondition::Fog,
            51..=57 => WeatherCondition::Drizzle,
            61..=67 | 80..=82 => WeatherCondition::Rain,
            71..=77 | 85..=86 => WeatherCondition::Snow,
            95..=99 => WeatherCondition::Thunderstorm,
            _ => WeatherCondition::Clear,
        }
    }

    pub fn condition_text(&self) -> &'static str {
        if !self.is_loaded {
            return "Offline";
        }
        match self.condition() {
            WeatherCondition::Clear => {
                if self.is_day {
                    "Sunny"
                } else {
                    "Clear"
                }
            }
            WeatherCondition::PartlyCloudy => "Partly Cloudy",
            WeatherCondition::Cloudy => "Overcast",
            WeatherCondition::Fog => "Foggy",
            WeatherCondition::Drizzle => "Drizzle",
            WeatherCondition::Rain => "Rain",
            WeatherCondition::Thunderstorm => "Thunderstorm",
            WeatherCondition::Snow => "Snow",
        }
    }

    #[allow(dead_code)]
    pub fn format_temp(&self) -> String {
        if !self.is_loaded {
            "--°C".to_string()
        } else {
            format!("{:.0}°C", self.temperature.round())
        }
    }

    pub fn format_temp_compact(&self) -> String {
        if !self.is_loaded {
            "--°".to_string()
        } else {
            format!("{:.0}°", self.temperature.round())
        }
    }

    #[allow(dead_code)]
    pub fn format_high_low(&self) -> String {
        if !self.is_loaded {
            "H: --°  L: --°".to_string()
        } else {
            format!("H: {:.0}°  L: {:.0}°", self.temp_max.round(), self.temp_min.round())
        }
    }

    pub fn is_stale(&self) -> bool {
        self.last_updated.elapsed() > Duration::from_secs(1800)
    }
}

impl Default for WeatherInfo {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            apparent_temperature: 0.0,
            weather_code: 0,
            is_day: true,
            relative_humidity: 0,
            wind_speed: 0.0,
            temp_max: 0.0,
            temp_min: 0.0,
            city: "Offline".to_string(),
            is_offline: true,
            is_loaded: false,
            last_updated: Instant::now(),
        }
    }
}

pub struct WeatherManager {
    info: Arc<RwLock<WeatherInfo>>,
    running: Arc<AtomicBool>,
    refresh_requested: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
    worker_thread: Arc<RwLock<Option<thread::Thread>>>,
}

impl WeatherManager {
    pub fn new(hwnd: HWND) -> Self {
        // 1. Instantly load persistent disk cache if available
        let (initial_info, init_lat, init_lon, init_city, init_geo) =
            if let Some((lat, lon, city, cached)) = load_cache_from_disk() {
                (cached, lat, lon, city, true)
            } else {
                (WeatherInfo::default(), -6.2, 106.8, "Jakarta".to_string(), false)
            };

        let info = Arc::new(RwLock::new(initial_info));
        let running = Arc::new(AtomicBool::new(true));
        let refresh_requested = Arc::new(AtomicBool::new(false));
        let worker_thread = Arc::new(RwLock::new(None));

        let info_clone = Arc::clone(&info);
        let running_clone = Arc::clone(&running);
        let refresh_requested_clone = Arc::clone(&refresh_requested);
        let worker_thread_clone = Arc::clone(&worker_thread);
        let hwnd_val = hwnd.0 as usize;

        let worker_handle = thread::Builder::new()
            .name("WeatherWorker".to_string())
            .spawn(move || {
                let hwnd = HWND(hwnd_val as *mut _);
                if let Ok(mut g) = worker_thread_clone.write() {
                    *g = Some(thread::current());
                }

                let mut cached_lat = init_lat;
                let mut cached_lon = init_lon;
                let mut cached_city = init_city;
                let mut geo_resolved = init_geo;

                while running_clone.load(Ordering::Relaxed) {
                    // 1. Geolocation lookup if not yet resolved
                    if !geo_resolved {
                        if let Ok(ip_json) = fetch_url_native("http://ip-api.com/json") {
                            if let (Some(lat), Some(lon)) = (
                                extract_json_num(&ip_json, "lat"),
                                extract_json_num(&ip_json, "lon"),
                            ) {
                                cached_lat = lat;
                                cached_lon = lon;
                                if let Some(city) = extract_json_str(&ip_json, "city") {
                                    if !city.is_empty() {
                                        cached_city = city.to_string();
                                    }
                                }
                                geo_resolved = true;
                            }
                        }
                    }

                    // 2. Fetch Open-Meteo weather
                    let url = format!(
                        "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,weather_code,wind_speed_10m&daily=temperature_2m_max,temperature_2m_min&timezone=auto",
                        cached_lat, cached_lon
                    );

                    let mut fetch_success = false;
                    if let Ok(weather_json) = fetch_url_native(&url) {
                        if let Some(mut parsed) = parse_open_meteo_json(&weather_json) {
                            parsed.city = cached_city.clone();
                            parsed.is_offline = false;
                            parsed.is_loaded = true;

                            // Persist successful online data to disk cache!
                            save_cache_to_disk(cached_lat, cached_lon, &cached_city, &parsed);

                            if let Ok(mut w) = info_clone.write() {
                                *w = parsed;
                            }
                            fetch_success = true;
                            unsafe {
                                let _ = PostMessageW(
                                    hwnd,
                                    WM_APP_WEATHER_UPDATED,
                                    WPARAM(0),
                                    LPARAM(0),
                                );
                            }
                        }
                    }

                    if !fetch_success {
                        // Connection failed / offline
                        if let Ok(mut w) = info_clone.write() {
                            w.is_offline = true;
                        }
                        unsafe {
                            let _ = PostMessageW(
                                hwnd,
                                WM_APP_WEATHER_UPDATED,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                        // Sleep only 15 seconds when offline so reconnect happens quickly!
                        for _ in 0..15 {
                            if !running_clone.load(Ordering::Relaxed) {
                                return;
                            }
                            if refresh_requested_clone.swap(false, Ordering::SeqCst) {
                                break;
                            }
                            thread::park_timeout(Duration::from_secs(1));
                        }
                    } else {
                        // Success: sleep 30 minutes (1800s) until next update, wakeable via unpark
                        for _ in 0..180 {
                            if !running_clone.load(Ordering::Relaxed) {
                                return;
                            }
                            if refresh_requested_clone.swap(false, Ordering::SeqCst) {
                                break;
                            }
                            thread::park_timeout(Duration::from_secs(10));
                        }
                    }
                }
            })
            .ok();

        Self {
            info,
            running,
            refresh_requested,
            worker_handle,
            worker_thread,
        }
    }

    pub fn get_weather(&self) -> Option<WeatherInfo> {
        self.info.read().ok().map(|g| g.clone())
    }

    pub fn trigger_refresh(&self) {
        self.refresh_requested.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.worker_thread.read() {
            if let Some(t) = guard.as_ref() {
                t.unpark();
            }
        }
    }
}

impl Drop for WeatherManager {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Ok(guard) = self.worker_thread.read() {
            if let Some(t) = guard.as_ref() {
                t.unpark();
            }
        }
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

fn get_cache_file_path() -> Option<PathBuf> {
    let app_data = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let island_dir = app_data.join("NotDynamicIsland");
    let _ = std::fs::create_dir_all(&island_dir);
    Some(island_dir.join("weather_cache.txt"))
}

fn save_cache_to_disk(lat: f32, lon: f32, city: &str, info: &WeatherInfo) {
    if let Some(path) = get_cache_file_path() {
        let content = format!(
            "city={}\nlat={}\nlon={}\ntemperature={}\napparent_temperature={}\nweather_code={}\nis_day={}\nrelative_humidity={}\nwind_speed={}\ntemp_max={}\ntemp_min={}\n",
            city,
            lat,
            lon,
            info.temperature,
            info.apparent_temperature,
            info.weather_code,
            if info.is_day { 1 } else { 0 },
            info.relative_humidity,
            info.wind_speed,
            info.temp_max,
            info.temp_min,
        );
        let _ = std::fs::write(path, content);
    }
}

fn load_cache_from_disk() -> Option<(f32, f32, String, WeatherInfo)> {
    let path = get_cache_file_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let mut city = "Current Location".to_string();
    let mut lat = -6.2;
    let mut lon = 106.8;
    let mut temperature = 26.0;
    let mut apparent_temperature = 26.0;
    let mut weather_code = 0;
    let mut is_day = true;
    let mut relative_humidity = 70;
    let mut wind_speed = 0.0;
    let mut temp_max = 30.0;
    let mut temp_min = 22.0;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            match k.trim() {
                "city" => city = v.trim().to_string(),
                "lat" => if let Ok(val) = v.trim().parse() { lat = val; },
                "lon" => if let Ok(val) = v.trim().parse() { lon = val; },
                "temperature" => if let Ok(val) = v.trim().parse() { temperature = val; },
                "apparent_temperature" => if let Ok(val) = v.trim().parse() { apparent_temperature = val; },
                "weather_code" => if let Ok(val) = v.trim().parse() { weather_code = val; },
                "is_day" => is_day = v.trim() == "1",
                "relative_humidity" => if let Ok(val) = v.trim().parse() { relative_humidity = val; },
                "wind_speed" => if let Ok(val) = v.trim().parse() { wind_speed = val; },
                "temp_max" => if let Ok(val) = v.trim().parse() { temp_max = val; },
                "temp_min" => if let Ok(val) = v.trim().parse() { temp_min = val; },
                _ => {}
            }
        }
    }

    let modified_time = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok();
    let is_stale = modified_time
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d > Duration::from_secs(1800))
        .unwrap_or(true);

    let last_updated = if is_stale {
        Instant::now().checked_sub(Duration::from_secs(1801)).unwrap_or_else(Instant::now)
    } else {
        Instant::now()
    };

    let info = WeatherInfo {
        temperature,
        apparent_temperature,
        weather_code,
        is_day,
        relative_humidity,
        wind_speed,
        temp_max,
        temp_min,
        city: city.clone(),
        is_offline: true,
        is_loaded: true,
        last_updated,
    };

    Some((lat, lon, city, info))
}

/// Zero-dependency, ultra-lightweight JSON numeric parser
pub fn extract_json_num(json: &str, key: &str) -> Option<f32> {
    let pattern = format!("\"{}\":", key);
    let idx = json.find(&pattern)?;
    let after = json[idx + pattern.len()..].trim_start();
    let end_idx = after
        .find(|c: char| !(c.is_numeric() || c == '.' || c == '-'))
        .unwrap_or(after.len());
    after[..end_idx].parse::<f32>().ok()
}

/// Zero-dependency, ultra-lightweight JSON string parser
pub fn extract_json_str<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("\"{}\":", key);
    let idx = json.find(&pattern)?;
    let after = json[idx + pattern.len()..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let after_quote = &after[1..];
    let quote_idx = after_quote.find('"')?;
    Some(&after_quote[..quote_idx])
}

/// Zero-dependency, ultra-lightweight parser for the first element in a JSON numeric array
pub fn extract_first_num_in_array(json: &str, key: &str) -> Option<f32> {
    let pattern = format!("\"{}\":", key);
    let idx = json.find(&pattern)?;
    let after = json[idx + pattern.len()..].trim_start();
    if !after.starts_with('[') {
        return None;
    }
    let after_bracket = after[1..].trim_start();
    let end_idx = after_bracket
        .find(|c: char| !(c.is_numeric() || c == '.' || c == '-'))
        .unwrap_or(after_bracket.len());
    after_bracket[..end_idx].parse::<f32>().ok()
}

/// Parses the Open-Meteo response into WeatherInfo without any external crate
pub fn parse_open_meteo_json(json: &str) -> Option<WeatherInfo> {
    let current_idx = json.find("\"current\":")?;
    let current_slice = &json[current_idx..];

    let temperature = extract_json_num(current_slice, "temperature_2m")?;
    let apparent_temperature = extract_json_num(current_slice, "apparent_temperature")
        .unwrap_or(temperature);
    let weather_code = extract_json_num(current_slice, "weather_code").unwrap_or(0.0) as u32;
    let is_day = extract_json_num(current_slice, "is_day").unwrap_or(1.0) >= 0.5;
    let relative_humidity = extract_json_num(current_slice, "relative_humidity_2m").unwrap_or(70.0) as u32;
    let wind_speed = extract_json_num(current_slice, "wind_speed_10m").unwrap_or(0.0);

    let temp_max = extract_first_num_in_array(json, "temperature_2m_max")
        .unwrap_or(temperature + 3.0);
    let temp_min = extract_first_num_in_array(json, "temperature_2m_min")
        .unwrap_or(temperature - 3.0);

    Some(WeatherInfo {
        temperature,
        apparent_temperature,
        weather_code,
        is_day,
        relative_humidity,
        wind_speed,
        temp_max,
        temp_min,
        city: "Current Location".to_string(),
        is_offline: false,
        is_loaded: true,
        last_updated: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_primitives() {
        let json = r#"{"city":"Jakarta","lat":-6.2146,"lon":106.8451,"temperature_2m_max":[33.5,32.1]}"#;
        assert_eq!(extract_json_str(json, "city"), Some("Jakarta"));
        assert_eq!(extract_json_num(json, "lat"), Some(-6.2146));
        assert_eq!(extract_json_num(json, "lon"), Some(106.8451));
        assert_eq!(extract_first_num_in_array(json, "temperature_2m_max"), Some(33.5));
    }

    #[test]
    fn test_parse_open_meteo_json_mock() {
        let sample = r#"{
            "latitude": -6.15,
            "longitude": 106.24,
            "current": {
                "temperature_2m": 26.2,
                "relative_humidity_2m": 96,
                "apparent_temperature": 32.8,
                "is_day": 0,
                "precipitation": 0.00,
                "weather_code": 3,
                "wind_speed_10m": 2.2
            },
            "daily": {
                "temperature_2m_max": [33.6, 32.7],
                "temperature_2m_min": [25.7, 25.0]
            }
        }"#;

        let info = parse_open_meteo_json(sample).expect("Should parse open meteo json");
        assert_eq!(info.temperature, 26.2);
        assert_eq!(info.apparent_temperature, 32.8);
        assert_eq!(info.weather_code, 3);
        assert_eq!(info.is_day, false);
        assert_eq!(info.relative_humidity, 96);
        assert_eq!(info.wind_speed, 2.2);
        assert_eq!(info.temp_max, 33.6);
        assert_eq!(info.temp_min, 25.7);
        assert_eq!(info.condition(), WeatherCondition::Cloudy);
        assert_eq!(info.condition_text(), "Overcast");
    }

    #[test]
    fn test_weather_cache_roundtrip() {
        let info = WeatherInfo {
            temperature: 26.5,
            apparent_temperature: 31.0,
            weather_code: 3,
            is_day: false,
            relative_humidity: 90,
            wind_speed: 3.5,
            temp_max: 33.0,
            temp_min: 24.0,
            city: "Ciruas".to_string(),
            is_offline: false,
            is_loaded: true,
            last_updated: Instant::now(),
        };

        save_cache_to_disk(-6.12, 106.23, "Ciruas", &info);
        let loaded = load_cache_from_disk();
        assert!(loaded.is_some());
        let (lat, lon, city, cached) = loaded.unwrap();
        assert_eq!(city, "Ciruas");
        assert_eq!(lat, -6.12);
        assert_eq!(lon, 106.23);
        assert_eq!(cached.temperature, 26.5);
        assert_eq!(cached.weather_code, 3);
        assert_eq!(cached.is_loaded, true);
        assert_eq!(cached.is_offline, true);
        assert_eq!(cached.is_stale(), false);
    }

    #[test]
    fn test_weather_stale_detection() {
        let mut info = WeatherInfo::default();
        assert_eq!(info.is_stale(), false);

        info.last_updated = Instant::now()
            .checked_sub(Duration::from_secs(1801))
            .unwrap_or_else(Instant::now);
        assert_eq!(info.is_stale(), true);
    }
}
