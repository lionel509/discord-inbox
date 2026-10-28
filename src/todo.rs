//! Todo files for the daily brief (#135). Stub: signatures only until the todo unit lands.
use chrono::NaiveDate;
use std::path::{Path, PathBuf};

pub const DIR: &str = "! Todo";
pub const CHECKIN_LOG: &str = "Stormy Check-ins.md";

#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Line { file: PathBuf, idx: usize },
    Note { file: PathBuf },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub text: String,
    pub place: Place,
}

pub fn day_file(_dir: &Path, _day: NaiveDate) -> PathBuf {
    todo!("todo::day_file (#135)")
}

pub fn day_items(_dir: &Path, _day: NaiveDate) -> Vec<Item> {
    todo!("todo::day_items (#135)")
}

pub fn note_items(_dir: &Path) -> Vec<Item> {
    todo!("todo::note_items (#135)")
}

pub fn mark_done(_item: &Item, _day: NaiveDate) -> Result<(), String> {
    todo!("todo::mark_done (#135)")
}

pub fn carry(_dir: &Path, _day: NaiveDate, _texts: &[String]) -> Result<usize, String> {
    todo!("todo::carry (#135)")
}

pub fn log_checkin(_dir: &Path, _day: NaiveDate, _reply: &str, _done: &[String], _carried: &[String]) -> Result<(), String> {
    todo!("todo::log_checkin (#135)")
}
