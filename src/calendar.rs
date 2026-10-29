//! Checks off a finished school-calendar event (#95): a DM like "done with math hw 3" renames the matching event to start with `✅ `. Never deletes, moves or creates an event. Only names listed in the calendars file ever reach `osascript`.
use crate::chat::Chat;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_EVENTS: usize = 60;
pub const SQLITE_TIMEOUT: Duration = Duration::from_secs(10);
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
pub const CALENDAR_TZ: &str = "America/New_York";
pub const DONE_MARK: &str = "✅ ";
// Never read, whatever the calendars file says. Matched on the trimmed, lowercased name.
const PRIVATE: &[&str] = &["home", "work", "important", "lionel"];

// Lowercase prefixes and suffixes only: a message must start or end with one, never merely contain it.
const PREFIXES: &[&str] = &[
    "/done ",
    "done with ",
    "done w ",
    "i'm done with ",
    "im done with ",
    "i finished ",
    "just finished ",
    "finished ",
    "i submitted ",
    "submitted ",
    "i turned in ",
    "turned in ",
];
const SUFFIXES: &[&str] = &[" done", " ✅", " - done"];
// A word right before a finish suffix that makes it not finished: "HW 3 not done", "well done", "almost done".
const NEGATIONS: &[&str] = &[
    "not", "never", "no", "yet", "almost", "nearly", "half", "well", "quite", "mostly", "partly", "barely", "hardly", "near", "been",
    "havent", "hasnt", "isnt", "arent", "wasnt", "didnt", "aint",
];

/// The task text when the message says something is finished, else `None`.
pub fn wanted(content: &str) -> Option<String> {
    // Curly apostrophes (iOS Smart Punctuation) become plain ones before anything else.
    let norm = content.replace('\u{2019}', "'");
    let t = norm.trim();
    // ASCII lowercase keeps every byte offset, so the slices below line up with `t`.
    let lower = t.to_ascii_lowercase();
    let rest = match PREFIXES.iter().filter(|p| lower.starts_with(*p)).max_by_key(|p| p.len()) {
        Some(p) => &t[p.len()..],
        None => {
            let s = SUFFIXES.iter().find(|s| lower.ends_with(*s))?;
            let head = &t[..t.len() - s.len()];
            if unfinished(head) {
                return None;
            }
            head
        }
    };
    let rest = rest.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

/// True when the last word before a suffix negates or qualifies the finish.
fn unfinished(head: &str) -> bool {
    let last = head
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
        .to_ascii_lowercase();
    NEGATIONS.contains(&last.as_str()) || last.ends_with("n't")
}

/// Calendar names from the calendars file: one per line, whole-line `#` comments and blanks skipped (a `#` inside a name is kept), leading BOMs stripped, duplicates dropped.
pub fn parse_calendars(text: &str) -> Vec<String> {
    // Editors can stack BOMs; only leading ones are stripped, as in allowlist.rs.
    let text = text.trim_start_matches('\u{feff}');
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || out.iter().any(|n| n == line) {
            continue;
        }
        out.push(line.to_string());
    }
    out
}

/// `$DISCORD_INBOX_CALENDARS_FILE` else `~/.config/discord-inbox/calendars`.
pub fn calendars_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DISCORD_INBOX_CALENDARS_FILE") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/discord-inbox/calendars")
}

/// `$DISCORD_INBOX_OSASCRIPT` if set, else `/usr/bin/osascript`.
pub fn osascript_path() -> PathBuf {
    match std::env::var_os("DISCORD_INBOX_OSASCRIPT") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from("/usr/bin/osascript"),
    }
}

/// `$DISCORD_INBOX_CALENDAR_DB` if set, else Calendar's store in its group container.
pub fn calendar_db_path() -> PathBuf {
    match std::env::var_os("DISCORD_INBOX_CALENDAR_DB") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join("Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb")
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cal {
    pub osascript: PathBuf,
    pub calendars: Vec<String>,
    pub db: PathBuf,
}

impl Cal {
    /// `None` when the file is missing or lists no calendars: the feature is off.
    pub fn load(path: &Path) -> Option<Cal> {
        let text = std::fs::read_to_string(path).ok()?;
        let calendars = parse_calendars(&text);
        (!calendars.is_empty()).then(|| Cal { osascript: osascript_path(), calendars, db: calendar_db_path() })
    }

    pub fn from_env() -> Option<Cal> {
        Cal::load(&calendars_path())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub uid: String,
    pub calendar: String,
    /// YYYY-MM-DD
    pub date: String,
    /// "HH:MM" or "HH:MM-HH:MM" (start-end, local; an end at or before the start is past midnight), None for all-day
    pub time: Option<String>,
    pub summary: String,
}

impl Event {
    /// True when the summary already starts with `✅`.
    pub fn done(&self) -> bool {
        self.summary.starts_with('✅')
    }
}

// The only write: prefixes `✅ ` once. Never delete, move or create.
pub const CHECK_SCRIPT: &str = r#"on run argv
	set calName to item 2 of argv
	set theUid to item 3 of argv
	tell application "Calendar"
		tell calendar calName
			set e to first event whose uid is theUid
			set s to summary of e
			if not (s starts with "✅") then set summary of e to "✅ " & s
			return summary of e
		end tell
	end tell
end run
"#;

/// One event per line: `uid TAB calendar TAB date TAB time|allday TAB summary`. The time sits before the summary so a tab inside the summary survives. Malformed lines are dropped.
pub fn parse_events(out: &str) -> Vec<Event> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(5, '\t').collect();
            (f.len() == 5 && !f[0].is_empty()).then(|| Event {
                uid: f[0].to_string(),
                calendar: f[1].to_string(),
                date: f[2].to_string(),
                time: (f[3] != "allday").then(|| f[3].to_string()),
                summary: f[4].to_string(),
            })
        })
        .collect()
}

/// The configured names that may be read: all of them except the private ones.
pub fn course_names(names: &[String]) -> Vec<String> {
    names.iter().filter(|n| !PRIVATE.contains(&n.trim().to_lowercase().as_str())).cloned().collect()
}

/// Two statements: the calendar titles that exist (`#cal` lines), then one row per cached occurrence of the named calendars. All-day rows take their date from `day` (local midnight), timed rows from `occurrence_date`. A timed event that crosses midnight has a spill-over row on the next day, which is dropped.
pub fn list_sql(names: &[String]) -> String {
    let names = names.iter().map(|n| format!("'{}'", n.replace('\'', "''"))).collect::<Vec<_>>().join(", ");
    format!(
        "SELECT '#cal', title FROM Calendar WHERE title IN ({names});
SELECT DISTINCT i.UUID, c.title,
  CASE WHEN i.all_day = 1 THEN date(o.day + 978307200, 'unixepoch', 'localtime') ELSE date(o.occurrence_date + 978307200, 'unixepoch', 'localtime') END AS d,
  CASE WHEN i.all_day = 1 THEN 'allday' ELSE strftime('%H:%M', o.occurrence_date + 978307200, 'unixepoch', 'localtime') || CASE WHEN o.occurrence_end_date > o.occurrence_date THEN '-' || strftime('%H:%M', o.occurrence_end_date + 978307200, 'unixepoch', 'localtime') ELSE '' END END AS t,
  replace(replace(ifnull(i.summary, ''), char(13), ' '), char(10), ' ')
FROM OccurrenceCache o JOIN CalendarItem i ON i.ROWID = o.event_id JOIN Calendar c ON c.ROWID = i.calendar_id
WHERE c.title IN ({names}) AND i.UUID IS NOT NULL AND (i.all_day = 1 OR o.occurrence_start_date IS NULL)
ORDER BY d, (t = 'allday') DESC, t, i.UUID;
"
    )
}

/// A temp directory holding the scratch copy. Removed on drop, on every path.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Copies the database, and its `-wal` and `-shm` when present, into a fresh temp directory. sqlite only ever opens the copy.
fn scratch_copy(db: &Path) -> Result<Scratch, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("discord-inbox-cal-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let scratch = Scratch(dir.clone());
    let unreadable = || format!("calendar database not readable: {}", db.display());
    std::fs::copy(db, dir.join("Calendar.sqlitedb")).map_err(|_| unreadable())?;
    for suffix in ["-wal", "-shm"] {
        let src = PathBuf::from(format!("{}{suffix}", db.display()));
        if src.exists() {
            std::fs::copy(&src, dir.join(format!("Calendar.sqlitedb{suffix}"))).map_err(|_| unreadable())?;
        }
    }
    Ok(scratch)
}

/// Events from the configured calendars, every cached occurrence in the store (no date window). Reads a scratch copy of Calendar's database with one read-only query; the live file is never opened.
pub fn list(cal: &Cal) -> Result<Vec<Event>, String> {
    let names = course_names(&cal.calendars);
    if names.is_empty() {
        return Err("no course calendars configured".into());
    }
    let scratch = scratch_copy(&cal.db)?;
    let copy = scratch.0.join("Calendar.sqlitedb");
    let args = [
        format!("TZ={CALENDAR_TZ}"),
        "/usr/bin/sqlite3".to_string(),
        "-batch".to_string(),
        "-bail".to_string(),
        "-noheader".to_string(),
        "-list".to_string(),
        "-separator".to_string(),
        "\t".to_string(),
        copy.display().to_string(),
    ];
    let out = crate::ocr::run_args(Path::new("/usr/bin/env"), &args, list_sql(&names).as_bytes(), SQLITE_TIMEOUT)
        .ok_or_else(|| "calendar database query failed, timed out, or none of the configured calendars exist".to_string())?;
    drop(scratch);
    let found: Vec<&str> = out.lines().filter_map(|l| l.strip_prefix("#cal\t")).collect();
    for n in &names {
        if !found.contains(&n.as_str()) {
            eprintln!("discord-inbox: calendar not found: {n}");
        }
    }
    let evs = parse_events(&out);
    eprintln!("discord-inbox: calendar list: {} events in {} calendars", evs.len(), cal.calendars.len());
    Ok(evs)
}

/// Prefixes the event's summary with `✅ `. Returns the summary Calendar.app now holds. Refuses any calendar not configured.
pub fn check_off(cal: &Cal, ev: &Event) -> Result<String, String> {
    if !cal.calendars.contains(&ev.calendar) {
        return Err("calendar is not configured".to_string());
    }
    let args = ["-".to_string(), "check".to_string(), ev.calendar.clone(), ev.uid.clone()];
    let out = crate::ocr::run_args(&cal.osascript, &args, CHECK_SCRIPT.as_bytes(), CHECK_TIMEOUT)
        .ok_or_else(|| "check-off failed or timed out".to_string())?;
    if !out.starts_with('✅') {
        return Err("check-off not confirmed".to_string());
    }
    eprintln!("discord-inbox: checked off {} in {}", ev.uid, ev.calendar);
    Ok(out)
}

/// Open events (not yet marked), nearest to `today` first, at most MAX_EVENTS.
pub fn candidates(events: &[Event], today: &str) -> Vec<Event> {
    let t = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok();
    let dist = |e: &Event| -> i64 {
        match (t, chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d")) {
            (Some(t), Ok(d)) => (d - t).num_days().abs(),
            _ => i64::MAX,
        }
    };
    let mut open: Vec<Event> = events.iter().filter(|e| !e.done()).cloned().collect();
    open.sort_by_key(|e| dist(e));
    open.truncate(MAX_EVENTS);
    open
}

/// (system, user) for the one-shot pick. The model answers with a tag only.
pub fn pick_prompt(message: &str, today: &str, open: &[Event]) -> (String, String) {
    let system = "You match one finished task to one calendar event. Reply with only the tag, like `#3`, of the single event the message says is finished. Reply `NONE` if no listed event clearly matches. Do not pick an event just because it is nearest.";
    let mut user = format!("Today: {today}\nMessage: {message}\n\nEvents:\n");
    for (i, e) in open.iter().enumerate() {
        user += &format!("#{} {} {}: {}\n", i + 1, e.date, e.calendar, e.summary);
    }
    (system.to_string(), user)
}

#[derive(Debug, PartialEq)]
pub enum Pick {
    One(usize),
    Many(Vec<usize>),
    None,
}

/// Every distinct `#<digits>` in the reply that is in 1..=n, as 0-based indexes. Anything else in the reply is ignored, and nothing defaults to `#1`.
pub fn parse_pick(reply: &str, n: usize) -> Pick {
    // NONE wins over any tag the reply also mentions ("NONE. #2 is closest but not it").
    if reply.split(|c: char| !c.is_ascii_alphabetic()).any(|w| w.eq_ignore_ascii_case("none")) {
        return Pick::None;
    }
    let bytes = reply.as_bytes();
    let mut picks: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'#' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if let Ok(v) = reply[start..j].parse::<usize>() {
            if (1..=n).contains(&v) && !picks.contains(&(v - 1)) {
                picks.push(v - 1);
            }
        }
        i = j.max(i + 1);
    }
    match picks.len() {
        0 => Pick::None,
        1 => Pick::One(picks[0]),
        _ => {
            picks.sort();
            Pick::Many(picks)
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Finish {
    Done(String),
    Ask(String),
    NoMatch,
}

/// Lists, asks the model for one pick, and checks off the pick. Several picks asks the user which one, and writes nothing.
pub fn finish(chat: &dyn Chat, cal: &Cal, message: &str, today: &str) -> Result<Finish, String> {
    let events = list(cal)?;
    // A uid listed more than once is a recurring series or a multi-day span: ticking it would rename every occurrence, so it is never offered.
    let events: Vec<Event> = events.iter().filter(|e| events.iter().filter(|x| x.uid == e.uid).count() == 1).cloned().collect();
    let open = candidates(&events, today);
    if open.is_empty() {
        return Ok(Finish::NoMatch);
    }
    // The whole message goes to the model, so "not done" reaches it intact.
    let (system, user) = pick_prompt(message, today, &open);
    let reply = chat.ask(&system, &user)?;
    match parse_pick(&reply, open.len()) {
        Pick::None => Ok(Finish::NoMatch),
        Pick::Many(ix) => {
            let mut text = String::from("Which one did you mean? 🐾");
            for &i in ix.iter().take(5) {
                let e = &open[i];
                text += &format!("\n• {} {} ({})", e.date, e.summary, e.calendar);
            }
            text += "\nSay `done with <title>` and I'll tick it.";
            Ok(Finish::Ask(text))
        }
        Pick::One(i) => {
            let ev = &open[i];
            let new = check_off(cal, ev)?;
            let summary = new.trim_start_matches('✅').trim();
            Ok(Finish::Done(format!("Checked off ✅ {summary} ({}, {}) 🎉", ev.calendar, ev.date)))
        }
    }
}
