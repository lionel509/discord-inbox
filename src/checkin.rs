//! Evening check-in rule (U3). At most one check-in per local day, and none on a day that already has a meal.
//! The per-day marker is written before the send, so a crash mid-send gives that day no check-in
//! (never-twice wins over always-once).
use crate::daylog;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use std::fs;
use std::path::{Path, PathBuf};

pub const HOUR: u32 = 21;
/// Cheap, energy-dense, one-pan suggestions. No banned food (see `BANNED`).
pub const SUGGESTIONS: &[&str] = &[
    "eggs scrambled in butter over rice with soy sauce",
    "peanut butter and honey toast with a glass of milk",
    "instant noodles with an egg and frozen veg",
    "tuna mayo rice bowl with cucumber",
    "pan-fried salmon over rice with cabbage",
    "oatmeal with peanut butter, banana and milk",
    "chicken thigh and frozen veg stir-fried in oil over rice",
    "tofu and egg fried rice with peas",
    "cheese quesadilla with avocado",
    "pasta with olive oil, garlic, parmesan and frozen spinach",
    "greek yogurt with granola, honey and nuts",
    "bean and cheese burrito",
];
pub const BANNED: &[&str] = &[
    "ginger", "wine", "mirin", "sake", "shaoxing", "beer", "liquor", "coffee", "decaf", "espresso", "tea", "matcha", "chocolate", "cocoa", "cola",
];

pub fn marker_path(dir: &Path, day: &str) -> PathBuf {
    dir.join(format!(".checkin-{day}"))
}

/// The day to check in on, or `None`. Due from `HOUR` on, once per day, and only when no meal is logged.
pub fn due(dir: &Path, now_local: NaiveDateTime) -> Option<String> {
    if now_local.hour() < HOUR {
        return None;
    }
    let day = now_local.date().format("%Y-%m-%d").to_string();
    if marker_path(dir, &day).exists() || daylog::has_meal(dir, &day) {
        return None;
    }
    Some(day)
}

/// True when more than 120 s passed since the last tick, which means the laptop slept in between.
/// Read from the wall clock: `Instant` does not advance while a Mac sleeps.
pub fn just_woke(prev: NaiveDateTime, now: NaiveDateTime) -> bool {
    now - prev > chrono::Duration::seconds(120)
}

pub fn text(day: &str) -> String {
    let ord = NaiveDate::parse_from_str(day, "%Y-%m-%d").map(|d| d.ordinal() as usize).unwrap_or(0);
    let pick = SUGGESTIONS[ord % SUGGESTIONS.len()];
    format!(
        "Hey, nothing logged today. Did you eat? Reply with what you had (weights if you have them, or a recipe name).\nIf not, something easy and filling: {pick}"
    )
}

/// Writes the marker first, then sends. A failed send removes the marker so the day can retry.
pub async fn send<F, Fut>(dir: &Path, day: &str, f: F) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let marker = marker_path(dir, day);
    fs::write(&marker, "sent\n").map_err(|e| e.to_string())?;
    match f(text(day)).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&marker);
            Err(e)
        }
    }
}
