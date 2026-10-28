//! Stormy's morning brief and night check-in (#135). Stub: signatures only until the daily unit lands.
use crate::calendar::{Cal, Event};
use crate::chat::Chat;
use crate::todo::Item;
use crate::weather::Weather;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const TICK: Duration = Duration::from_secs(60);
pub const GRACE_MIN: i64 = 90;
pub const DEFAULT_MORNING: &str = "08:00";
pub const DEFAULT_NIGHT: &str = "22:30";
pub const AHEAD_DAYS: i64 = 3;
pub const MAX_CHARS: usize = 1900;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slot {
    Morning,
    Night,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Times {
    pub morning: NaiveTime,
    pub night: NaiveTime,
}

pub struct Deps {
    pub todo_dir: PathBuf,
    pub cal: Option<Cal>,
    pub chat: Arc<dyn Chat>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub checkin: bool,
    pub done: Vec<usize>,
    pub tomorrow: Vec<String>,
}

pub fn parse_time(_s: &str) -> Option<NaiveTime> {
    todo!("daily::parse_time (#135)")
}

pub fn times_from_env() -> Times {
    todo!("daily::times_from_env (#135)")
}

pub fn marker_path(_inbox: &Path, _slot: Slot, _day: &str) -> PathBuf {
    todo!("daily::marker_path (#135)")
}

pub fn due(_inbox: &Path, _times: &Times, _now: NaiveDateTime) -> Option<(Slot, String)> {
    todo!("daily::due (#135)")
}

pub fn stage(_inbox: &Path, _slot: Slot, _day: &str) -> Result<(), String> {
    todo!("daily::stage (#135)")
}

pub fn awaiting(_inbox: &Path, _now: NaiveDateTime) -> Option<String> {
    todo!("daily::awaiting (#135)")
}

pub fn answered(_inbox: &Path, _day: &str) -> Result<(), String> {
    todo!("daily::answered (#135)")
}

pub fn is_deadline(_ev: &Event) -> bool {
    todo!("daily::is_deadline (#135)")
}

pub fn is_exam(_ev: &Event) -> bool {
    todo!("daily::is_exam (#135)")
}

pub fn morning_facts(_todo_dir: &Path, _events: &[Event], _weather: Option<&Weather>, _today: NaiveDate) -> String {
    todo!("daily::morning_facts (#135)")
}

pub fn night_facts(_todo_dir: &Path, _events: &[Event], _today: NaiveDate) -> String {
    todo!("daily::night_facts (#135)")
}

pub fn voice(_chat: &dyn Chat, _slot: Slot, _facts: &str) -> String {
    todo!("daily::voice (#135)")
}

pub fn compose(_deps: &Deps, _slot: Slot, _today: NaiveDate) -> String {
    todo!("daily::compose (#135)")
}

pub fn answer_prompt(_items: &[Item], _reply: &str, _day: NaiveDate) -> (String, String) {
    todo!("daily::answer_prompt (#135)")
}

pub fn parse_answer(_text: &str, _n: usize) -> Option<Answer> {
    todo!("daily::parse_answer (#135)")
}

pub fn apply(_todo_dir: &Path, _day: NaiveDate, _items: &[Item], _a: &Answer) -> Result<(Vec<String>, Vec<String>), String> {
    todo!("daily::apply (#135)")
}

pub fn answer(_deps: &Deps, _day: NaiveDate, _reply: &str) -> Result<Option<String>, String> {
    todo!("daily::answer (#135)")
}
