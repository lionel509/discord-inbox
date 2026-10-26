//! Evening check-in rule (U3). At most one check-in per local day, and none on a day that already has a meal.
//! Every allowlisted id gets each day's check-in at least once (I3). Each id's marker is written after its send, so a
//! failed send is retried on the next tick, and only a crash between a successful send and its marker write repeats
//! that one DM, once.
use crate::daylog;
use crate::rich::Rich;
use crate::summary;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use std::fs;
use std::future::Future;
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

/// Sends the check-in to one recipient, then writes the day marker on success. A failed send leaves no marker, so the
/// day can retry.
pub async fn send<F, Fut>(dir: &Path, day: &str, f: F) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    f(text(day)).await?;
    fs::write(marker_path(dir, day), "sent\n").map_err(|e| e.to_string())
}

/// Sends the check-in to each id, at least once (I3), through `summary::send_each`: each id's marker is written after
/// its send, so a failed id is retried on the next tick and the ids that already got it are skipped. The day marker is
/// written only when every id has been sent. A failed send is returned to the caller.
pub async fn send_all<F, Fut>(dir: &Path, day: &str, ids: &[u64], mut send_one: F) -> Result<(), String>
where
    F: FnMut(u64, String) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let body = text(day);
    let marker = marker_path(dir, day);
    let name = marker.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let base = marker.with_file_name(format!("{name}.dm"));
    summary::send_each(&base, ids, |uid| send_one(uid, body.clone())).await?;
    fs::write(&marker, "sent\n").map_err(|e| e.to_string())
}

/// One pass of the check-in loop: the check-in, the daily card and the weekly card, each per recipient (I3: every id
/// gets each once, a failed id is retried on the next pass). Returns the log lines; the bin prints them.
pub async fn tick<F, Fut>(
    dir: &Path,
    now: NaiveDateTime,
    ids: &[u64],
    low_kcal: u32,
    kcal_target: u32,
    mut send: F,
) -> Vec<String>
where
    F: FnMut(u64, Rich) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let mut log = Vec::new();
    if let Some(day) = due(dir, now) {
        let r = send_all(dir, &day, ids, |uid, body| send(uid, Rich { text: body, ..Default::default() })).await;
        log.push(match r {
            Ok(()) => format!("check-in sent for {day}"),
            Err(e) => format!("check-in for {day} failed: {e}"),
        });
    }
    if let Some(day) = summary::daily_due(dir, now) {
        let card = summary::day_card(dir, &day, low_kcal, kcal_target);
        let marker = summary::marker_path(dir, &day);
        let r = summary::send_each(&marker, ids, |uid| send(uid, card.clone())).await;
        log.push(match r {
            Ok(()) => format!("summary sent for {day}"),
            Err(e) => format!("summary for {day} failed: {e}"),
        });
    }
    if let Some(day) = summary::weekly_due(dir, now) {
        let card = summary::week_card(dir, &day, low_kcal, kcal_target);
        let marker = summary::weekly_marker(dir, &day);
        let r = summary::send_each(&marker, ids, |uid| send(uid, card.clone())).await;
        log.push(match r {
            Ok(()) => format!("weekly sent for {day}"),
            Err(e) => format!("weekly for {day} failed: {e}"),
        });
    }
    log
}
