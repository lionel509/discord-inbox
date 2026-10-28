// Morning weather (#135): Open-Meteo parse, the wear list, the one-line brief, and the file override. No network: fetch reads the sample file. No rain line, on purpose (his phone alerts on rain).
use discord_inbox::weather::{self, Weather};
use std::path::PathBuf;

fn sample_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/weather_sample.json")
}

fn sample() -> Weather {
    Weather { now_f: 52, hi_f: 61, lo_f: 45, code: 61 }
}

fn w(lo: i32, code: u16) -> Weather {
    Weather { now_f: 52, hi_f: 61, lo_f: lo, code }
}

#[test]
fn parse_reads_the_sample_and_rejects_garbage() {
    let text = std::fs::read_to_string(sample_path()).unwrap();
    assert_eq!(weather::parse(&text), Some(sample()));
    assert_eq!(weather::parse("{\"current\": {}}"), None);
}

#[test]
fn fetch_reads_the_override_file() {
    let good = std::env::temp_dir().join(format!("di-weather-good-{}.json", std::process::id()));
    let bad = std::env::temp_dir().join(format!("di-weather-bad-{}.json", std::process::id()));
    std::fs::write(&good, std::fs::read_to_string(sample_path()).unwrap()).unwrap();
    std::fs::write(&bad, "garbage").unwrap();
    std::env::set_var("DISCORD_INBOX_WEATHER_FILE", &good);
    assert_eq!(weather::fetch(), Some(sample()));
    std::env::set_var("DISCORD_INBOX_WEATHER_FILE", &bad);
    assert_eq!(weather::fetch(), None);
}

#[test]
fn wear_follows_the_temperature_and_snow_only() {
    assert_eq!(weather::wear(&w(38, 0)), vec!["coat and layers"]);
    assert_eq!(weather::wear(&w(50, 0)), vec!["jacket"]);
    assert_eq!(weather::wear(&w(60, 0)), vec!["hoodie"]);
    assert_eq!(weather::wear(&w(70, 0)), vec!["t-shirt weather"]);
    // Rain never adds an umbrella item: the phone alerts on rain already.
    assert_eq!(weather::wear(&w(50, 61)), vec!["jacket"]);
    assert_eq!(weather::wear(&w(50, 71)), vec!["jacket", "boots"]);
}

#[test]
fn line_is_one_sentence_with_no_rain_percent() {
    let line = weather::line(&sample());
    assert_eq!(line, "Weather (Stony Brook): 52°F now, high 61 / low 45, rain");
    assert!(!line.contains('%'), "{line}");
}

#[test]
fn describe_names_the_sky() {
    assert_eq!(weather::describe(0), "clear");
    assert_eq!(weather::describe(2), "cloudy");
    assert_eq!(weather::describe(61), "rain");
    assert_eq!(weather::describe(71), "snow");
    assert_eq!(weather::describe(95), "thunderstorms");
}
