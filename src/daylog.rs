//! Day notes (U3). Stub: every function is todo!().
use crate::nutrition::Macros;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Meal {
    pub msg_id: u64,
    pub time: String,
    pub lines: Vec<String>,
    pub macros: Macros,
    pub veg: bool,
    pub recipe: Option<String>,
    pub unknown: Vec<String>,
    pub note: Option<String>,
    /// Saved photo file names (not paths); each renders as `![[name]]`.
    pub photos: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum Written {
    Logged,
    Already,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Totals {
    pub macros: Macros,
    pub meals: usize,
    pub veg: bool,
    pub flags: Vec<&'static str>,
}

pub fn day_key(_received: &str) -> &str {
    todo!()
}
pub fn note_path(_dir: &Path, _day: &str) -> PathBuf {
    todo!()
}
pub fn logged(_dir: &Path, _day: &str, _msg_id: u64) -> bool {
    todo!()
}
pub fn has_meal(_dir: &Path, _day: &str) -> bool {
    todo!()
}
pub fn write_meal(_dir: &Path, _day: &str, _meal: &Meal, _low_kcal: u32) -> std::io::Result<Written> {
    todo!()
}
pub fn totals(_dir: &Path, _day: &str, _low_kcal: u32) -> Option<Totals> {
    todo!()
}
pub fn photo_path(_dir: &Path, _day: &str, _hhmm: &str, _msg_id: u64, _i: usize, _name: &str) -> PathBuf {
    todo!()
}
pub fn save_photo(_path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
    todo!()
}
pub fn low_kcal_threshold() -> u32 {
    todo!()
}
