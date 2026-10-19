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

/// The task text when the message says something is finished, else `None`.
pub fn wanted(content: &str) -> Option<String> {
    let t = content.trim();
    // ASCII lowercase keeps every byte offset, so the slices below line up with `t`.
    let lower = t.to_ascii_lowercase();
    let rest = match PREFIXES.iter().filter(|p| lower.starts_with(*p)).max_by_key(|p| p.len()) {
        Some(p) => &t[p.len()..],
        None => {
            let s = SUFFIXES.iter().find(|s| lower.ends_with(*s))?;
            &t[..t.len() - s.len()]
        }
    };
    let rest = rest.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Calendar names from the calendars file: one per line, `#` comments and blanks skipped, leading BOMs stripped, duplicates dropped.
pub fn parse_calendars(text: &str) -> Vec<String> {
    // Editors can stack BOMs; only leading ones are stripped, as in allowlist.rs.
    let text = text.trim_start_matches('\u{feff}');
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() || out.iter().any(|n| n == line) {
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

#[derive(Clone, Debug, PartialEq)]
pub struct Cal {
    pub osascript: PathBuf,
    pub calendars: Vec<String>,
}

impl Cal {
    /// `None` when the file is missing or lists no calendars: the feature is off.
    pub fn load(path: &Path) -> Option<Cal> {
        let text = std::fs::read_to_string(path).ok()?;
        let calendars = parse_calendars(&text);
        (!calendars.is_empty()).then(|| Cal { osascript: osascript_path(), calendars })
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
    pub summary: String,
}

impl Event {
    /// True when the summary already starts with `✅`.
    pub fn done(&self) -> bool {
        self.summary.starts_with('✅')
    }
}

// The day window (PAST_DAYS, AHEAD_DAYS) is written out here: keep it in step with the consts above.
// Read-only: it prints one line per event and the first line is the "ok" sentinel, so an empty window still gives output.
pub const LIST_SCRIPT: &str = r#"on pad2(n)
	return text -2 thru -1 of ("0" & n)
end pad2

on run argv
	set wantedNames to items 2 thru -1 of argv
	set startWindow to (current date) - (21 * days)
	set endWindow to (current date) + (45 * days)
	set tabChar to ASCII character 9
	set out to {"ok"}
	tell application "Calendar"
		repeat with calName in wantedNames
			try
				repeat with ev in (every event of calendar calName whose start date >= startWindow and start date <= endWindow)
					set d to start date of ev
					set iso to ((year of d) as text) & "-" & my pad2((month of d) as integer) & "-" & my pad2(day of d)
					set end of out to (uid of ev) & tabChar & (calName as text) & tabChar & iso & tabChar & (summary of ev)
				end repeat
			end try
		end repeat
	end tell
	set AppleScript's text item delimiters to linefeed
	set res to out as text
	set AppleScript's text item delimiters to ""
	return res
end run
"#;

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

/// One event per line: `uid TAB calendar TAB date TAB summary`. The summary keeps any tabs it has. Malformed lines are dropped.
pub fn parse_events(out: &str) -> Vec<Event> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(4, '\t').collect();
            (f.len() == 4 && !f[0].is_empty()).then(|| Event {
                uid: f[0].to_string(),
                calendar: f[1].to_string(),
                date: f[2].to_string(),
                summary: f[3].to_string(),
            })
        })
        .collect()
}

/// Upcoming and recent events from the configured calendars only.
pub fn list(cal: &Cal) -> Result<Vec<Event>, String> {
    let mut args = vec!["-".to_string(), "list".to_string()];
    args.extend(cal.calendars.iter().cloned());
    let out = crate::ocr::run_args(&cal.osascript, &args, LIST_SCRIPT.as_bytes(), LIST_TIMEOUT)
        .ok_or_else(|| "osascript list failed or timed out".to_string())?;
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
pub fn pick_prompt(task: &str, today: &str, open: &[Event]) -> (String, String) {
    let system = "You match one finished task to one calendar event. Reply with only the tag, like `#3`, of the single event the message says is finished. Reply `NONE` if no listed event clearly matches. Do not pick an event just because it is nearest.";
    let mut user = format!("Today: {today}\nMessage: {task}\n\nEvents:\n");
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
pub fn finish(chat: &dyn Chat, cal: &Cal, task: &str, today: &str) -> Result<Finish, String> {
    let events = list(cal)?;
    let open = candidates(&events, today);
    if open.is_empty() {
        return Ok(Finish::NoMatch);
    }
    let (system, user) = pick_prompt(task, today, &open);
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
