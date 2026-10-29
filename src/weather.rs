//! Morning weather for Stony Brook (#135). No rain line on purpose: his phone already alerts on rain.
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

/// Reads the Open-Meteo JSON shape (current temperature, today's daily max/min/code).
/// Any missing field gives None.
pub fn parse(json: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let now = v.get("current")?.get("temperature_2m")?.as_f64()?;
    let daily = v.get("daily")?;
    let hi = daily.get("temperature_2m_max")?.get(0)?.as_f64()?;
    let lo = daily.get("temperature_2m_min")?.get(0)?.as_f64()?;
    let code = daily.get("weather_code")?.get(0)?.as_u64()?;
    Some(Weather {
        now_f: now.round() as i32,
        hi_f: hi.round() as i32,
        lo_f: lo.round() as i32,
        code: u16::try_from(code).ok()?,
    })
}

/// DISCORD_INBOX_WEATHER_FILE, when set, is read instead of the network (tests and demos).
/// Otherwise a blocking GET of URL. Any failure gives None.
pub fn fetch() -> Option<Weather> {
    if let Some(path) = std::env::var_os("DISCORD_INBOX_WEATHER_FILE") {
        return match std::fs::read_to_string(&path) {
            Ok(text) => parse(&text),
            Err(e) => {
                eprintln!("discord-inbox: weather: {e}");
                None
            }
        };
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());
    let text = client
        .get(URL)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text());
    match text {
        Ok(body) => parse(&body),
        Err(e) => {
            eprintln!("discord-inbox: weather: {e}");
            None
        }
    }
}

/// WMO weather code to a word for the sky.
pub fn describe(code: u16) -> &'static str {
    match code {
        0 => "clear",
        1..=3 => "cloudy",
        45..=48 => "fog",
        51..=67 => "rain",
        71..=77 => "snow",
        80..=82 => "showers",
        95..=99 => "thunderstorms",
        _ => "mixed",
    }
}

/// What to wear, from the low temperature and snow. Rain adds nothing (no umbrella item).
pub fn wear(w: &Weather) -> Vec<String> {
    let item = if w.lo_f < 40 {
        "coat and layers"
    } else if w.lo_f < 55 {
        "jacket"
    } else if w.lo_f < 65 {
        "hoodie"
    } else {
        "t-shirt weather"
    };
    let mut out = vec![item.to_string()];
    if (71..=77).contains(&w.code) {
        out.push("boots".to_string());
    }
    out
}

/// The one-line brief. No rain percentage.
pub fn line(w: &Weather) -> String {
    format!(
        "Weather (Stony Brook): {}°F now, high {} / low {}, {}",
        w.now_f,
        w.hi_f,
        w.lo_f,
        describe(w.code)
    )
}
