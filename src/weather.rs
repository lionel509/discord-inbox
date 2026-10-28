//! Morning weather for Stony Brook (#135): Open-Meteo parse, the wear list and the one-line brief. No rain line, on purpose (his phone alerts on rain).
use std::time::Duration;

pub const URL: &str = "https://api.open-meteo.com/v1/forecast?latitude=40.9257&longitude=-73.1409&timezone=America%2FNew_York&forecast_days=1&temperature_unit=fahrenheit&current=temperature_2m&daily=temperature_2m_max,temperature_2m_min,weather_code";
pub const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    pub now_f: i32,
    pub hi_f: i32,
    pub lo_f: i32,
    pub code: u16,
}

/// Reads the Open-Meteo forecast JSON. `None` when any field is missing.
pub fn parse(json: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let now = v["current"]["temperature_2m"].as_f64()?;
    let hi = v["daily"]["temperature_2m_max"][0].as_f64()?;
    let lo = v["daily"]["temperature_2m_min"][0].as_f64()?;
    let code = v["daily"]["weather_code"][0].as_u64()?;
    Some(Weather { now_f: now.round() as i32, hi_f: hi.round() as i32, lo_f: lo.round() as i32, code: u16::try_from(code).ok()? })
}

/// `$DISCORD_INBOX_WEATHER_FILE` (a saved forecast) when set, else the live forecast. `None` on any failure.
pub fn fetch() -> Option<Weather> {
    if let Some(p) = std::env::var_os("DISCORD_INBOX_WEATHER_FILE") {
        return parse(&std::fs::read_to_string(p).ok()?);
    }
    let client = reqwest::blocking::Client::builder().timeout(TIMEOUT).build().ok()?;
    let body = client.get(URL).send().ok()?.error_for_status().ok()?.text().ok()?;
    parse(&body)
}

pub fn describe(code: u16) -> &'static str {
    match code {
        0 => "clear",
        1..=3 => "cloudy",
        45 | 48 => "fog",
        51..=67 => "rain",
        71..=77 => "snow",
        80..=82 => "showers",
        95..=99 => "thunderstorms",
        _ => "mixed skies",
    }
}

pub fn wear(w: &Weather) -> Vec<String> {
    let base = match w.lo_f {
        i32::MIN..=39 => "coat and layers",
        40..=54 => "jacket",
        55..=64 => "hoodie",
        _ => "t-shirt weather",
    };
    let mut out = vec![base.to_string()];
    if (71..=77).contains(&w.code) {
        out.push("boots".to_string());
    }
    out
}

pub fn line(w: &Weather) -> String {
    format!("Weather (Stony Brook): {}°F now, high {} / low {}, {}", w.now_f, w.hi_f, w.lo_f, describe(w.code))
}
