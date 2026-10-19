//! Checks off a finished school-calendar event (#95): a DM like "done with math hw 3" renames the matching event to start with `✅ `. Never deletes, moves or creates an event. Only names listed in the calendars file ever reach `osascript`.
use crate::chat::Chat;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PAST_DAYS: i64 = 21;
pub const AHEAD_DAYS: i64 = 45;
pub const MAX_EVENTS: usize = 60;
pub const LIST_TIMEOUT: Duration = Duration::from_secs(25);
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
pub const DONE_MARK: &str = "✅ ";

/// The task text when the message says something is finished, else `None`. Placeholder until the change lands.
pub fn wanted(_content: &str) -> Option<String> {
    todo!()
}

/// Calendar names from the calendars file: one per line, `#` comments and blanks skipped, leading BOMs stripped, duplicates dropped.
pub fn parse_calendars(_text: &str) -> Vec<String> {
    todo!()
}

/// `$DISCORD_INBOX_CALENDARS_FILE` else `~/.config/discord-inbox/calendars`.
pub fn calendars_path() -> PathBuf {
    todo!()
}

/// `$DISCORD_INBOX_OSASCRIPT` if set, else `/usr/bin/osascript`.
pub fn osascript_path() -> PathBuf {
    todo!()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cal {
    pub osascript: PathBuf,
    pub calendars: Vec<String>,
}

impl Cal {
    /// `None` when the file is missing or lists no calendars: the feature is off.
    pub fn load(_path: &Path) -> Option<Cal> {
        todo!()
    }

    /// `load` on `calendars_path()`, with `osascript_path()`.
    pub fn from_env() -> Option<Cal> {
        todo!()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub uid: String,
    pub calendar: String,
    /// YYYY-MM-DD
    pub date: String,
    pub summary: String,
}

impl Event {
    /// True when the summary already starts with `✅`.
    pub fn done(&self) -> bool {
        todo!()
    }
}

/// AppleScript: argv is `list <calendar names…>`; prints one `uid TAB calendar TAB date TAB summary` line per event in the window.
pub const LIST_SCRIPT: &str = "";

/// AppleScript: argv is `check <calendar> <uid>`; prefixes the summary with `✅ ` once and prints it.
pub const CHECK_SCRIPT: &str = "";

pub fn parse_events(_out: &str) -> Vec<Event> {
    todo!()
}

pub fn list(_cal: &Cal) -> Result<Vec<Event>, String> {
    todo!()
}

pub fn check_off(_cal: &Cal, _ev: &Event) -> Result<String, String> {
    todo!()
}

pub fn candidates(_events: &[Event], _today: &str) -> Vec<Event> {
    todo!()
}

/// (system, user) for the one-shot pick.
pub fn pick_prompt(_task: &str, _today: &str, _open: &[Event]) -> (String, String) {
    todo!()
}

#[derive(Debug, PartialEq)]
pub enum Pick {
    One(usize),
    Many(Vec<usize>),
    None,
}

pub fn parse_pick(_reply: &str, _n: usize) -> Pick {
    todo!()
}

#[derive(Debug, PartialEq)]
pub enum Finish {
    Done(String),
    Ask(String),
    NoMatch,
}

pub fn finish(_chat: &dyn Chat, _cal: &Cal, _task: &str, _today: &str) -> Result<Finish, String> {
    todo!()
}
