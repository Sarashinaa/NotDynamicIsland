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
    manual_override_active: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
    worker_thread: Arc<RwLock<Option<thread::Thread>>>,
}

struct ComMtaGuard(bool);

impl ComMtaGuard {
    fn init() -> Self {
        let hr = unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            )
        };
        Self(hr.is_ok())
    }
}

impl Drop for ComMtaGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

enum GeocodeOutcome {
    Found(f32, f32, String),
    NotFound,
    NetworkError,
}

impl WeatherManager {
    pub fn new(hwnd: HWND) -> Self {
        // 1. Instantly load persistent disk cache if available for 0ms startup display
        let (initial_info, init_lat, init_lon, init_city, init_has_coords) =
            if let Some((lat, lon, city, cached)) = load_cache_from_disk() {
                (cached, lat, lon, city, true)
            } else {
                (WeatherInfo::default(), 0.0, 0.0, String::new(), false)
            };

        let info = Arc::new(RwLock::new(initial_info));
        let running = Arc::new(AtomicBool::new(true));
        let refresh_requested = Arc::new(AtomicBool::new(false));
        let manual_override_active = Arc::new(AtomicBool::new(load_manual_location().is_some()));
        let worker_thread = Arc::new(RwLock::new(None));

        let info_clone = Arc::clone(&info);
        let running_clone = Arc::clone(&running);
        let refresh_requested_clone = Arc::clone(&refresh_requested);
        let manual_override_clone = Arc::clone(&manual_override_active);
        let worker_thread_clone = Arc::clone(&worker_thread);
        let hwnd_val = hwnd.0 as usize;

        let worker_handle = thread::Builder::new()
            .name("WeatherWorker".to_string())
            .spawn(move || {
                let _com_guard = ComMtaGuard::init();
                let hwnd = HWND(hwnd_val as *mut _);
                if let Ok(mut g) = worker_thread_clone.write() {
                    *g = Some(thread::current());
                }

                let mut cached_lat = init_lat;
                let mut cached_lon = init_lon;
                let mut cached_city = init_city;
                let mut has_valid_coords = init_has_coords;
                // Always re-resolve live geolocation on startup so location is accurate and never stale!
                let mut geo_resolved = false;

                while running_clone.load(Ordering::Relaxed) {
                    let mut manual_blocking_fetch = false;
                    let mut resolved_coords_this_cycle = false;

                    // 1. Tiered Geolocation lookup if not yet resolved or if refresh requested
                    if !geo_resolved {
                        // Tier 0: Explicit user-pinned manual city override
                        if let Some(mut manual) = load_manual_location() {
                            manual_override_clone.store(true, Ordering::SeqCst);
                            if let (Some(lat), Some(lon), Some(city)) =
                                (manual.lat, manual.lon, manual.resolved_city.as_ref())
                            {
                                cached_lat = lat;
                                cached_lon = lon;
                                cached_city = city.clone();
                                geo_resolved = true;
                                has_valid_coords = true;
                            } else {
                                match geocode_manual_query(&manual.query) {
                                    GeocodeOutcome::Found(lat, lon, city) => {
                                        manual.lat = Some(lat);
                                        manual.lon = Some(lon);
                                        manual.resolved_city = Some(city.clone());
                                        save_manual_location(&manual);
                                        cached_lat = lat;
                                        cached_lon = lon;
                                        cached_city = city;
                                        geo_resolved = true;
                                        has_valid_coords = true;
                                    }
                                    GeocodeOutcome::NotFound => {
                                        remove_manual_location_file();
                                        manual_override_clone.store(false, Ordering::SeqCst);
                                    }
                                    GeocodeOutcome::NetworkError => {
                                        manual_blocking_fetch = true;
                                    }
                                }
                            }
                        } else {
                            manual_override_clone.store(false, Ordering::SeqCst);
                        }

                        // Tier 1: Windows Native Geolocation (Wi-Fi / GPS / Cell; rejects IP/Unknown/Obfuscated)
                        if !geo_resolved && !manual_override_clone.load(Ordering::SeqCst) {
                            if let Some((lat, lon)) = query_windows_geolocation() {
                                cached_lat = lat;
                                cached_lon = lon;
                                has_valid_coords = true;
                                resolved_coords_this_cycle = true;

                                let rev_url = format!(
                                    "https://api.bigdatacloud.net/data/reverse-geocode-client?latitude={:.4}&longitude={:.4}&localityLanguage=en",
                                    lat, lon
                                );
                                if let Ok(rev_json) = fetch_url_native(&rev_url) {
                                    if let Some(city) = parse_bigdatacloud_city(&rev_json) {
                                        cached_city = city;
                                        geo_resolved = true;
                                    }
                                }
                                if !geo_resolved && cached_city.is_empty() {
                                    cached_city = "Current Location".to_string();
                                }
                            }
                        }

                        // Tier 2: Public IP Geolocation Fallback (ip-api HTTP -> ipwho.is HTTPS)
                        if !geo_resolved
                            && !manual_override_clone.load(Ordering::SeqCst)
                            && !resolved_coords_this_cycle
                        {
                            let ip_res = fetch_url_native("http://ip-api.com/json")
                                .ok()
                                .and_then(|j| parse_ip_api_json(&j))
                                .or_else(|| {
                                    fetch_url_native("https://ipwho.is/")
                                        .ok()
                                        .and_then(|j| parse_ipwho_json(&j))
                                });
                            if let Some((lat, lon, city)) = ip_res {
                                cached_lat = lat;
                                cached_lon = lon;
                                cached_city = city;
                                geo_resolved = true;
                                has_valid_coords = true;
                            }
                        }
                    }

                    // 2. Fetch Open-Meteo weather only if we have valid resolved coordinates
                    let mut fetch_success = false;
                    if has_valid_coords && !manual_blocking_fetch {
                        let url = format!(
                            "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,weather_code,wind_speed_10m&daily=temperature_2m_max,temperature_2m_min&timezone=auto",
                            cached_lat, cached_lon
                        );

                        if let Ok(weather_json) = fetch_url_native(&url) {
                            if let Some(mut parsed) = parse_open_meteo_json(&weather_json) {
                                parsed.city = if cached_city.is_empty() {
                                    "Current Location".to_string()
                                } else {
                                    cached_city.clone()
                                };
                                parsed.is_offline = false;
                                parsed.is_loaded = true;

                                // Persist successful online data to disk cache!
                                save_cache_to_disk(cached_lat, cached_lon, &parsed.city, &parsed);

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
                    }

                    if !fetch_success {
                        // Connection failed / offline / geolocation unavailable
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
                                geo_resolved = false;
                                break;
                            }
                            thread::park_timeout(Duration::from_secs(1));
                        }
                    } else {
                        // Success: sleep 30 minutes (1800s) until next update, wakeable via unpark
                        let mut manual_wake = false;
                        for _ in 0..180 {
                            if !running_clone.load(Ordering::Relaxed) {
                                return;
                            }
                            if refresh_requested_clone.swap(false, Ordering::SeqCst) {
                                geo_resolved = false;
                                manual_wake = true;
                                break;
                            }
                            thread::park_timeout(Duration::from_secs(10));
                        }
                        if !manual_wake {
                            geo_resolved = false;
                        }
                    }
                }
            })
            .ok();

        Self {
            info,
            running,
            refresh_requested,
            manual_override_active,
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

    pub fn set_manual_location(&self, city: &str) {
        let trimmed = city.trim();
        if is_auto_location_keyword(trimmed) {
            self.clear_manual_location();
            return;
        }
        let loc = ManualLocation {
            query: trimmed.to_string(),
            resolved_city: None,
            lat: None,
            lon: None,
        };
        save_manual_location(&loc);
        self.manual_override_active.store(true, Ordering::SeqCst);
        self.trigger_refresh();
    }

    pub fn clear_manual_location(&self) {
        remove_manual_location_file();
        self.manual_override_active.store(false, Ordering::SeqCst);
        self.trigger_refresh();
    }

    pub fn is_manual_location(&self) -> bool {
        self.manual_override_active.load(Ordering::SeqCst)
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

fn get_app_data_dir() -> PathBuf {
    let app_data = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let island_dir = app_data.join("NotDynamicIsland");
    let _ = std::fs::create_dir_all(&island_dir);
    island_dir
}

fn get_cache_file_path() -> Option<PathBuf> {
    Some(get_app_data_dir().join("weather_cache.txt"))
}

fn get_location_override_path() -> Option<PathBuf> {
    Some(get_app_data_dir().join("weather_location.txt"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManualLocation {
    pub query: String,
    pub resolved_city: Option<String>,
    pub lat: Option<f32>,
    pub lon: Option<f32>,
}

pub fn is_auto_location_keyword(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "" | "auto" | "otomatis" | "reset"
    )
}

fn load_manual_location_from_path(path: &std::path::Path) -> Option<ManualLocation> {
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed_all = content.trim();
    if trimmed_all.is_empty() {
        return None;
    }

    if !trimmed_all.contains('=') {
        let first_line = trimmed_all.lines().map(str::trim).find(|l| !l.is_empty())?;
        if is_auto_location_keyword(first_line) {
            return None;
        }
        return Some(ManualLocation {
            query: first_line.to_string(),
            resolved_city: None,
            lat: None,
            lon: None,
        });
    }

    let mut query = String::new();
    let mut resolved_city: Option<String> = None;
    let mut lat: Option<f32> = None;
    let mut lon: Option<f32> = None;

    for line in content.lines() {
        if let Some((k, v)) = line.trim().split_once('=') {
            let val = v.trim();
            match k.trim() {
                "query" => query = val.to_string(),
                "city" => {
                    if !val.is_empty() {
                        resolved_city = Some(val.to_string());
                    }
                }
                "lat" => {
                    if let Ok(parsed) = val.parse::<f32>() {
                        if parsed.is_finite() {
                            lat = Some(parsed);
                        }
                    }
                }
                "lon" => {
                    if let Ok(parsed) = val.parse::<f32>() {
                        if parsed.is_finite() {
                            lon = Some(parsed);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if query.is_empty() {
        if let Some(ref c) = resolved_city {
            query = c.clone();
        } else {
            return None;
        }
    }
    if is_auto_location_keyword(&query) {
        return None;
    }

    if let (Some(lt), Some(ln)) = (lat, lon) {
        if !is_valid_coords(lt, ln) {
            lat = None;
            lon = None;
        }
    } else {
        lat = None;
        lon = None;
    }

    Some(ManualLocation {
        query,
        resolved_city,
        lat,
        lon,
    })
}

fn save_manual_location_to_path(path: &std::path::Path, loc: &ManualLocation) {
    let mut content = format!("query={}\n", loc.query);
    if let Some(ref city) = loc.resolved_city {
        content.push_str(&format!("city={}\n", city));
    }
    if let (Some(lat), Some(lon)) = (loc.lat, loc.lon) {
        content.push_str(&format!("lat={}\nlon={}\n", lat, lon));
    }
    let _ = std::fs::write(path, content);
}

fn load_manual_location() -> Option<ManualLocation> {
    let path = get_location_override_path()?;
    load_manual_location_from_path(&path)
}

fn save_manual_location(loc: &ManualLocation) {
    if let Some(path) = get_location_override_path() {
        save_manual_location_to_path(&path, loc);
    }
}

fn remove_manual_location_file() {
    if let Some(path) = get_location_override_path() {
        let _ = std::fs::remove_file(path);
    }
}

fn query_windows_geolocation() -> Option<(f32, f32)> {
    use windows::Devices::Geolocation::{Geolocator, PositionAccuracy, PositionSource};
    use windows::Foundation::TimeSpan;

    let geolocator = Geolocator::new().ok()?;
    let _ = geolocator.SetDesiredAccuracy(PositionAccuracy::Default);
    let max_age = TimeSpan {
        Duration: 600 * 10_000_000,
    };
    let timeout = TimeSpan {
        Duration: 4 * 10_000_000,
    };
    let pos = geolocator
        .GetGeopositionAsyncWithAgeAndTimeout(max_age, timeout)
        .ok()?
        .get()
        .ok()?;
    let coord = pos.Coordinate().ok()?;
    let source = coord.PositionSource().ok()?;

    let accepted = matches!(
        source,
        PositionSource::WiFi
            | PositionSource::Satellite
            | PositionSource::Cellular
            | PositionSource::Default
    );
    if !accepted {
        return None;
    }

    if source != PositionSource::Default {
        if let Ok(acc) = coord.Accuracy() {
            if acc.is_finite() && acc > 25_000.0 {
                return None;
            }
        }
    }

    let point = coord.Point().ok()?.Position().ok()?;
    let lat = point.Latitude as f32;
    let lon = point.Longitude as f32;
    if is_valid_coords(lat, lon) {
        Some((lat, lon))
    } else {
        None
    }
}

fn geocode_city_open_meteo(query: &str) -> GeocodeOutcome {
    let encoded = url_encode_query(query);
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
        encoded
    );
    match fetch_url_native(&url) {
        Ok(json) => {
            if let Some((lat, lon, name)) = parse_open_meteo_geocoding_json(&json) {
                GeocodeOutcome::Found(lat, lon, name)
            } else if json.contains("\"generationtime_ms\"") {
                GeocodeOutcome::NotFound
            } else {
                GeocodeOutcome::NetworkError
            }
        }
        Err(_) => GeocodeOutcome::NetworkError,
    }
}

fn geocode_city_nominatim(query: &str) -> GeocodeOutcome {
    let encoded = url_encode_query(query);
    let url = format!(
        "https://nominatim.openstreetmap.org/search?q={}&format=jsonv2&addressdetails=1&limit=1",
        encoded
    );
    match fetch_url_native(&url) {
        Ok(json) => {
            if let Some((lat, lon, city)) = parse_nominatim_search_json(&json) {
                GeocodeOutcome::Found(lat, lon, city)
            } else if json.trim_start().starts_with('[') {
                GeocodeOutcome::NotFound
            } else {
                GeocodeOutcome::NetworkError
            }
        }
        Err(_) => GeocodeOutcome::NetworkError,
    }
}

fn geocode_manual_query(query: &str) -> GeocodeOutcome {
    let trimmed = query.trim();
    if trimmed.contains(',') {
        match geocode_city_nominatim(trimmed) {
            GeocodeOutcome::Found(lat, lon, city) => GeocodeOutcome::Found(lat, lon, city),
            GeocodeOutcome::NotFound => {
                let head = trimmed.split(',').next().unwrap_or("").trim();
                if !head.is_empty() && head != trimmed {
                    geocode_city_open_meteo(head)
                } else {
                    GeocodeOutcome::NotFound
                }
            }
            GeocodeOutcome::NetworkError => GeocodeOutcome::NetworkError,
        }
    } else {
        match geocode_city_open_meteo(trimmed) {
            GeocodeOutcome::Found(lat, lon, city) => GeocodeOutcome::Found(lat, lon, city),
            GeocodeOutcome::NotFound => geocode_city_nominatim(trimmed),
            GeocodeOutcome::NetworkError => match geocode_city_nominatim(trimmed) {
                GeocodeOutcome::Found(lat, lon, city) => GeocodeOutcome::Found(lat, lon, city),
                _ => GeocodeOutcome::NetworkError,
            },
        }
    }
}

fn save_cache_to_path(path: &std::path::Path, lat: f32, lon: f32, city: &str, info: &WeatherInfo) {
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

fn save_cache_to_disk(lat: f32, lon: f32, city: &str, info: &WeatherInfo) {
    if let Some(path) = get_cache_file_path() {
        save_cache_to_path(&path, lat, lon, city, info);
    }
}

fn load_cache_from_path(path: &std::path::Path) -> Option<(f32, f32, String, WeatherInfo)> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut city = "Current Location".to_string();
    let mut lat: Option<f32> = None;
    let mut lon: Option<f32> = None;
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
                "city" => {
                    let val = v.trim();
                    if !val.is_empty() {
                        city = val.to_string();
                    }
                }
                "lat" => if let Ok(val) = v.trim().parse() { lat = Some(val); },
                "lon" => if let Ok(val) = v.trim().parse() { lon = Some(val); },
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

    let (lat, lon) = (lat?, lon?);
    if !is_valid_coords(lat, lon) {
        return None;
    }

    let modified_time = std::fs::metadata(path)
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

fn load_cache_from_disk() -> Option<(f32, f32, String, WeatherInfo)> {
    let path = get_cache_file_path()?;
    load_cache_from_path(&path)
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

/// Zero-dependency, ultra-lightweight JSON boolean parser
pub fn extract_json_bool(json: &str, key: &str) -> Option<bool> {
    let pattern = format!("\"{}\":", key);
    let idx = json.find(&pattern)?;
    let after = json[idx + pattern.len()..].trim_start();
    if after.starts_with("true") {
        Some(true)
    } else if after.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

pub fn is_valid_coords(lat: f32, lon: f32) -> bool {
    lat.is_finite()
        && lon.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
        && (lat != 0.0 || lon != 0.0)
}

pub fn url_encode_query(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for b in s.trim().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0x0F) as usize] as char);
            }
        }
    }
    out
}

pub fn parse_ip_api_json(json: &str) -> Option<(f32, f32, String)> {
    if extract_json_str(json, "status") != Some("success") {
        return None;
    }
    let lat = extract_json_num(json, "lat")?;
    let lon = extract_json_num(json, "lon")?;
    if !is_valid_coords(lat, lon) {
        return None;
    }
    let city = extract_json_str(json, "city")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            extract_json_str(json, "regionName")
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("Current Location");
    Some((lat, lon, city.to_string()))
}

pub fn parse_ipwho_json(json: &str) -> Option<(f32, f32, String)> {
    if extract_json_bool(json, "success") != Some(true) {
        return None;
    }
    let lat = extract_json_num(json, "latitude")?;
    let lon = extract_json_num(json, "longitude")?;
    if !is_valid_coords(lat, lon) {
        return None;
    }
    let city = extract_json_str(json, "city")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            extract_json_str(json, "region")
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("Current Location");
    Some((lat, lon, city.to_string()))
}

pub fn parse_bigdatacloud_city(json: &str) -> Option<String> {
    for key in ["city", "locality", "principalSubdivision"] {
        if let Some(val) = extract_json_str(json, key).map(str::trim).filter(|s| !s.is_empty()) {
            return Some(val.to_string());
        }
    }
    None
}

pub fn parse_open_meteo_geocoding_json(json: &str) -> Option<(f32, f32, String)> {
    let idx = json.find("\"results\":")?;
    let slice = &json[idx..];
    let lat = extract_json_num(slice, "latitude")?;
    let lon = extract_json_num(slice, "longitude")?;
    if !is_valid_coords(lat, lon) {
        return None;
    }
    let name = extract_json_str(slice, "name")
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some((lat, lon, name.to_string()))
}

pub fn parse_nominatim_search_json(json: &str) -> Option<(f32, f32, String)> {
    let lat = extract_json_str(json, "lat")?.trim().parse::<f32>().ok()?;
    let lon = extract_json_str(json, "lon")?.trim().parse::<f32>().ok()?;
    if !is_valid_coords(lat, lon) {
        return None;
    }

    let addr_city = json.find("\"address\":").and_then(|addr_idx| {
        let rest = &json[addr_idx + "\"address\":".len()..];
        let brace_rel = rest.find('{')?;
        let obj = &rest[brace_rel..];
        let mut depth = 0usize;
        let mut in_str = false;
        let mut escape = false;
        let mut end_idx = obj.len();
        for (i, b) in obj.bytes().enumerate() {
            if in_str {
                if escape {
                    escape = false;
                } else if b == b'\\' {
                    escape = true;
                } else if b == b'"' {
                    in_str = false;
                }
                continue;
            }
            match b {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end_idx = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let addr_slice = &obj[..end_idx];
        for key in [
            "city",
            "town",
            "municipality",
            "village",
            "suburb",
            "county",
            "state",
        ] {
            if let Some(val) = extract_json_str(addr_slice, key)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(val.to_string());
            }
        }
        None
    });

    let display = addr_city
        .or_else(|| {
            extract_json_str(json, "name")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Current Location".to_string());

    Some((lat, lon, display))
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

    let daily_slice = json.find("\"daily\":").map(|idx| &json[idx..]).unwrap_or(json);
    let temp_max = extract_first_num_in_array(daily_slice, "temperature_2m_max")
        .unwrap_or(temperature + 3.0);
    let temp_min = extract_first_num_in_array(daily_slice, "temperature_2m_min")
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
            "latitude": 35.68,
            "longitude": 139.69,
            "current_units": {
                "temperature_2m": "°C",
                "relative_humidity_2m": "%"
            },
            "current": {
                "temperature_2m": 26.2,
                "relative_humidity_2m": 96,
                "apparent_temperature": 32.8,
                "is_day": 0,
                "precipitation": 0.00,
                "weather_code": 3,
                "wind_speed_10m": 2.2
            },
            "daily_units": {
                "time": "iso8601",
                "temperature_2m_max": "°C",
                "temperature_2m_min": "°C"
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
            city: "Tokyo".to_string(),
            is_offline: false,
            is_loaded: true,
            last_updated: Instant::now(),
        };

        let temp_test_file = std::env::temp_dir().join("ndi_weather_cache_unit_test.txt");
        save_cache_to_path(&temp_test_file, 35.68, 139.69, "Tokyo", &info);
        let loaded = load_cache_from_path(&temp_test_file);
        let _ = std::fs::remove_file(&temp_test_file);
        assert!(loaded.is_some());
        let (lat, lon, city, cached) = loaded.unwrap();
        assert_eq!(city, "Tokyo");
        assert_eq!(lat, 35.68);
        assert_eq!(lon, 139.69);
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

    #[test]
    fn test_parse_ip_api_json() {
        let valid = r#"{"status":"success","regionName":"Harbor Province","city":"Harborview","lat":34.6937,"lon":135.5023}"#;
        assert_eq!(
            parse_ip_api_json(valid),
            Some((34.6937, 135.5023, "Harborview".to_string()))
        );

        let fallback_region = r#"{"status":"success","regionName":"Alpine Region","city":"","lat":47.3769,"lon":8.5417}"#;
        assert_eq!(
            parse_ip_api_json(fallback_region),
            Some((47.3769, 8.5417, "Alpine Region".to_string()))
        );

        let failed = r#"{"status":"fail","message":"rate limited"}"#;
        assert_eq!(parse_ip_api_json(failed), None);

        let captive_html = "<html><body>Captive Portal Login</body></html>";
        assert_eq!(parse_ip_api_json(captive_html), None);
    }

    #[test]
    fn test_parse_ipwho_json() {
        let valid = "{\n  \"success\": true,\n  \"region\": \"Kansai\",\n  \"city\": \"Osaka\",\n  \"latitude\": 34.6937,\n  \"longitude\": 135.5023\n}";
        assert_eq!(
            parse_ipwho_json(valid),
            Some((34.6937, 135.5023, "Osaka".to_string()))
        );

        let failed = r#"{"success":false,"message":"invalid ip"}"#;
        assert_eq!(parse_ipwho_json(failed), None);
    }

    #[test]
    fn test_parse_bigdatacloud_city() {
        let with_city = r#"{"city":"Zurich","locality":"Altstadt","principalSubdivision":"Zurich Canton"}"#;
        assert_eq!(parse_bigdatacloud_city(with_city), Some("Zurich".to_string()));

        let with_locality = r#"{"city":"","locality":"North Harbor","principalSubdivision":"Bay State"}"#;
        assert_eq!(parse_bigdatacloud_city(with_locality), Some("North Harbor".to_string()));

        let with_subdiv = r#"{"city":"","locality":"","principalSubdivision":"Midland"}"#;
        assert_eq!(parse_bigdatacloud_city(with_subdiv), Some("Midland".to_string()));

        assert_eq!(parse_bigdatacloud_city(r#"{"city":""}"#), None);
    }

    #[test]
    fn test_parse_open_meteo_geocoding_json() {
        let valid = r#"{"results":[{"id":1,"name":"Metropolis","latitude":35.6895,"longitude":139.6917,"country":"Testland"}],"generationtime_ms":0.25}"#;
        assert_eq!(
            parse_open_meteo_geocoding_json(valid),
            Some((35.6895, 139.6917, "Metropolis".to_string()))
        );

        let empty = r#"{"generationtime_ms":0.18}"#;
        assert_eq!(parse_open_meteo_geocoding_json(empty), None);

        let empty_arr = r#"{"results":[],"generationtime_ms":0.18}"#;
        assert_eq!(parse_open_meteo_geocoding_json(empty_arr), None);
    }

    #[test]
    fn test_url_encode_query() {
        assert_eq!(url_encode_query("New York"), "New%20York");
        assert_eq!(url_encode_query("  Zurich-North_1.0~ "), "Zurich-North_1.0~");
        assert_eq!(url_encode_query("São Paulo"), "S%C3%A3o%20Paulo");
    }

    #[test]
    fn test_manual_location_roundtrip() {
        let temp_file = std::env::temp_dir().join("ndi_weather_location_unit_test.txt");

        let loc = ManualLocation {
            query: "Metropolis".to_string(),
            resolved_city: Some("Metropolis".to_string()),
            lat: Some(35.6895),
            lon: Some(139.6917),
        };
        save_manual_location_to_path(&temp_file, &loc);
        let loaded = load_manual_location_from_path(&temp_file);
        assert_eq!(loaded, Some(loc));

        let _ = std::fs::write(&temp_file, "  Osaka  \n");
        assert_eq!(
            load_manual_location_from_path(&temp_file),
            Some(ManualLocation {
                query: "Osaka".to_string(),
                resolved_city: None,
                lat: None,
                lon: None,
            })
        );

        let _ = std::fs::write(&temp_file, "auto\n");
        assert_eq!(load_manual_location_from_path(&temp_file), None);

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_parse_nominatim_search_json() {
        let landmark_with_city = r#"[{"place_id":1,"lat":"35.689500","lon":"139.691700","name":"Central Hall","address":{"amenity":"Central Hall","village":"Old Ward","city":"Metropolis","state":"Metro Region"}}]"#;
        assert_eq!(
            parse_nominatim_search_json(landmark_with_city),
            Some((35.6895, 139.6917, "Metropolis".to_string()))
        );

        let town_only = r#"[{"place_id":2,"lat":"47.376900","lon":"8.541700","name":"Northwood Station","address":{"town":"Northwood","county":"Alpine County","state":"Highland"}}]"#;
        assert_eq!(
            parse_nominatim_search_json(town_only),
            Some((47.3769, 8.5417, "Northwood".to_string()))
        );

        let no_address = r#"[{"place_id":3,"lat":"34.693700","lon":"135.502300","name":"Harborview"}]"#;
        assert_eq!(
            parse_nominatim_search_json(no_address),
            Some((34.6937, 135.5023, "Harborview".to_string()))
        );

        assert_eq!(parse_nominatim_search_json("[]"), None);
        assert_eq!(parse_nominatim_search_json("<html>Rate Limited</html>"), None);
    }
}
