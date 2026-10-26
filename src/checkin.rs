//! Evening check-in rule (U3). Stub: functions are todo!(); constants hold the spec values where they are fixed.
use std::path::{Path, PathBuf};

pub const HOUR: u32 = 21;
/// Filled in by U3; empty here so the suggestion tests stay red.
pub const SUGGESTIONS: &[&str] = &[];
pub const BANNED: &[&str] = &[
    "ginger", "wine", "mirin", "sake", "shaoxing", "beer", "liquor", "coffee", "decaf", "espresso", "tea", "matcha", "chocolate", "cocoa", "cola",
];

pub fn marker_path(_dir: &Path, _day: &str) -> PathBuf {
    todo!()
}
pub fn due(_dir: &Path, _now_local: chrono::NaiveDateTime) -> Option<String> {
    todo!()
}
pub fn text(_day: &str) -> String {
    todo!()
}
pub async fn send<F, Fut>(_dir: &Path, _day: &str, _f: F) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    todo!()
}
