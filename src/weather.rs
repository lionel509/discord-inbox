//! Morning weather for Stony Brook (#135). Stub: signatures only until the weather unit lands.
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

pub fn parse(_json: &str) -> Option<Weather> {
    todo!("weather::parse (#135)")
}

pub fn fetch() -> Option<Weather> {
    todo!("weather::fetch (#135)")
}

pub fn describe(_code: u16) -> &'static str {
    todo!("weather::describe (#135)")
}

pub fn wear(_w: &Weather) -> Vec<String> {
    todo!("weather::wear (#135)")
}

pub fn line(_w: &Weather) -> String {
    todo!("weather::line (#135)")
}
