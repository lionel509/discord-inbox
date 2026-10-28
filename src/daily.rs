//! Stormy's morning brief and night check-in (#135): the slot clock, the facts built in code, one model pass for voice, and the night reply.
use crate::calendar::{Cal, Event};
use crate::chat::Chat;
use crate::todo::{self, Item, Place};
use crate::weather::Weather;
use chrono::{Days, NaiveDate, NaiveDateTime, NaiveTime};
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

pub fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

pub fn times_from_env() -> Times {
    let get = |var: &str, default: &str| {
        std::env::var(var).ok().and_then(|v| parse_time(&v)).unwrap_or_else(|| parse_time(default).expect("default time"))
    };
    Times { morning: get("DISCORD_INBOX_MORNING_AT", DEFAULT_MORNING), night: get("DISCORD_INBOX_NIGHT_AT", DEFAULT_NIGHT) }
}

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Morning => "morning",
        Slot::Night => "night",
    }
}

/// Dot-file in the inbox, not a note: `.daily-morning-YYYY-MM-DD`. Holds `sent`, or `answered` for the night slot.
pub fn marker_path(inbox: &Path, slot: Slot, day: &str) -> PathBuf {
    inbox.join(format!(".daily-{}-{day}", slot_name(slot)))
}

fn today_str(now: NaiveDateTime) -> String {
    now.date().format("%Y-%m-%d").to_string()
}

/// The slot to send now: its time has passed, it is inside the grace window, and no marker exists for today.
pub fn due(inbox: &Path, times: &Times, now: NaiveDateTime) -> Option<(Slot, String)> {
    let day = today_str(now);
    for (slot, t) in [(Slot::Morning, times.morning), (Slot::Night, times.night)] {
        let start = now.date().and_time(t);
        let mins = (now - start).num_minutes();
        if (0..GRACE_MIN).contains(&mins) && !marker_path(inbox, slot, &day).exists() {
            return Some((slot, day));
        }
    }
    None
}

/// Writes the `sent` marker. Called before the DM goes out, so a crash never double-sends.
pub fn stage(inbox: &Path, slot: Slot, day: &str) -> Result<(), String> {
    std::fs::create_dir_all(inbox).map_err(|e| e.to_string())?;
    std::fs::write(marker_path(inbox, slot, day), "sent\n").map_err(|e| e.to_string())
}

/// The day whose night check-in was sent and not yet answered: today's, or yesterday's until 08:00.
pub fn awaiting(inbox: &Path, now: NaiveDateTime) -> Option<String> {
    let sent = |d: NaiveDate| {
        let day = d.format("%Y-%m-%d").to_string();
        let on = std::fs::read_to_string(marker_path(inbox, Slot::Night, &day)).map(|s| s.trim() == "sent").unwrap_or(false);
        on.then_some(day)
    };
    if let Some(d) = sent(now.date()) {
        return Some(d);
    }
    if now.time() < NaiveTime::from_hms_opt(8, 0, 0)? {
        return sent(now.date().checked_sub_days(Days::new(1))?);
    }
    None
}

pub fn answered(inbox: &Path, day: &str) -> Result<(), String> {
    std::fs::write(marker_path(inbox, Slot::Night, day), "answered\n").map_err(|e| e.to_string())
}

pub fn is_deadline(ev: &Event) -> bool {
    if ev.done() {
        return false;
    }
    let s = ev.summary.to_lowercase();
    s.contains("due") || s.contains("deadline") || s.contains("submit")
}

pub fn is_exam(ev: &Event) -> bool {
    !ev.done() && ev.calendar.to_lowercase().contains("exam")
}

fn on(events: &[Event], day: NaiveDate) -> Vec<&Event> {
    let d = day.format("%Y-%m-%d").to_string();
    let mut v: Vec<&Event> = events.iter().filter(|e| e.date == d && !e.done()).collect();
    // All-day first, then by clock time.
    v.sort_by(|a, b| a.time.cmp(&b.time));
    v
}

fn when(ev: &Event) -> String {
    ev.time.clone().unwrap_or_else(|| "all day".to_string())
}

fn open_items(todo_dir: &Path, day: NaiveDate) -> Vec<Item> {
    let mut v = todo::day_items(todo_dir, day);
    v.extend(todo::note_items(todo_dir));
    v
}

pub fn morning_facts(todo_dir: &Path, events: &[Event], weather: Option<&Weather>, today: NaiveDate) -> String {
    let mut out: Vec<String> = Vec::new();
    match weather {
        Some(w) => {
            out.push(crate::weather::line(w));
            out.push(format!("Wear: {}", crate::weather::wear(w).join(", ")));
        }
        None => out.push("Weather: unavailable".to_string()),
    }
    let todays = on(events, today);
    out.push(if todays.iter().any(|e| is_exam(e)) { "Bring: calculator (exam today)".to_string() } else { "Bring: nothing extra".to_string() });
    let items = todo::day_items(todo_dir, today);
    if todays.is_empty() && items.is_empty() {
        out.push("Today: nothing on the list".to_string());
    } else {
        out.push("Today:".to_string());
        for e in &todays {
            let w = if e.time.is_some() { when(e) } else { format!("{}:", when(e)) };
            out.push(format!("- {w} {} ({})", e.summary, e.calendar));
        }
        for i in &items {
            out.push(format!("- (todo) {}", i.text));
        }
    }
    let mut soon: Vec<String> = Vec::new();
    for n in 1..=AHEAD_DAYS {
        if let Some(d) = today.checked_add_days(Days::new(n as u64)) {
            for e in on(events, d).into_iter().filter(|e| is_deadline(e)) {
                soon.push(format!("- {} ({}) on {}", e.summary, e.calendar, d.format("%a %m/%d")));
            }
        }
    }
    if !soon.is_empty() {
        out.push("Due soon:".to_string());
        out.extend(soon);
    }
    out.join("\n")
}

pub fn night_facts(todo_dir: &Path, events: &[Event], today: NaiveDate) -> String {
    let mut out: Vec<String> = Vec::new();
    let tomorrow = today.checked_add_days(Days::new(1)).unwrap_or(today);
    let tm = on(events, tomorrow);
    // `on` sorts all-day first; the first timed event is the real first thing.
    let first = tm.iter().find(|e| e.time.is_some()).or_else(|| tm.first());
    out.push(match first {
        Some(e) => format!("Tomorrow first: {} {} ({})", e.time.clone().unwrap_or_else(|| "all day".to_string()), e.summary, e.calendar),
        None => "Tomorrow first: nothing scheduled".to_string(),
    });
    let due_tm: Vec<&&Event> = tm.iter().filter(|e| is_deadline(e)).collect();
    if !due_tm.is_empty() {
        out.push("Due tomorrow:".to_string());
        for e in due_tm {
            out.push(format!("- {} ({})", e.summary, e.calendar));
        }
    }
    let items = open_items(todo_dir, today);
    if items.is_empty() {
        out.push("Left on your list: nothing".to_string());
    } else {
        out.push("Left on your list:".to_string());
        for (i, it) in items.iter().enumerate() {
            out.push(format!("{}. {}", i + 1, it.text));
        }
    }
    out.join("\n")
}

/// One model pass to say the facts in Stormy's voice. Any failure or a too-short answer keeps the plain facts.
pub fn voice(chat: &dyn Chat, slot: Slot, facts: &str) -> String {
    let job = match slot {
        Slot::Morning => "Write Lionel's short good-morning text from these facts.",
        Slot::Night => "Write Lionel's short night check-in text from these facts, and ask what he got done and what is left for tomorrow.",
    };
    let system = format!("{}\n\n{job} Keep every fact, number and time exactly. Add nothing new. No markdown headings.", crate::vaults::PERSONA);
    match chat.ask(&system, &crate::redact::sensitive(facts)) {
        Ok(text) if text.trim().chars().count() * 2 >= facts.chars().count() => text.trim().to_string(),
        _ => facts.to_string(),
    }
}

pub fn compose(deps: &Deps, slot: Slot, today: NaiveDate) -> String {
    let events = match &deps.cal {
        Some(cal) => crate::calendar::list(cal).unwrap_or_else(|e| {
            eprintln!("discord-inbox: daily: calendar unavailable: {e}");
            Vec::new()
        }),
        None => Vec::new(),
    };
    let facts = match slot {
        Slot::Morning => morning_facts(&deps.todo_dir, &events, crate::weather::fetch().as_ref(), today),
        Slot::Night => night_facts(&deps.todo_dir, &events, today),
    };
    let text = voice(deps.chat.as_ref(), slot, &facts);
    if text.chars().count() > MAX_CHARS {
        text.chars().take(MAX_CHARS).collect()
    } else {
        text
    }
}

pub fn answer_prompt(items: &[Item], reply: &str, day: NaiveDate) -> (String, String) {
    let system = "Lionel answered his night check-in. Decide whether the message is a check-in reply (what he did or what is left), and answer with JSON only: {\"checkin\": bool, \"done\": [numbers from the list he finished], \"tomorrow\": [new short tasks he said he needs to do tomorrow]}. Only use numbers from the list. If the message is something else, checkin is false.".to_string();
    let list: Vec<String> = items.iter().enumerate().map(|(i, it)| format!("{}. {}", i + 1, it.text)).collect();
    let user = format!("Date: {}\nList:\n{}\n\nHis message:\n{}", day.format("%Y-%m-%d"), if list.is_empty() { "(empty)".to_string() } else { list.join("\n") }, crate::redact::sensitive(reply));
    (system, user)
}

pub fn parse_answer(text: &str, n: usize) -> Option<Answer> {
    let (a, b) = (text.find('{')?, text.rfind('}')?);
    let v: serde_json::Value = serde_json::from_str(text.get(a..=b)?).ok()?;
    let checkin = v["checkin"].as_bool()?;
    let mut done: Vec<usize> = Vec::new();
    for x in v["done"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        if let Some(k) = x.as_u64().map(|k| k as usize) {
            if k >= 1 && k <= n && !done.contains(&k) {
                done.push(k);
            }
        }
    }
    let tomorrow = v["tomorrow"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| t.as_str()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
        .unwrap_or_default();
    Some(Answer { checkin, done, tomorrow })
}

/// Strikes the done items, then carries the unfinished day-file lines plus the new tasks to tomorrow. Returns (done, carried) texts.
pub fn apply(todo_dir: &Path, day: NaiveDate, items: &[Item], a: &Answer) -> Result<(Vec<String>, Vec<String>), String> {
    let mut done = Vec::new();
    for &k in &a.done {
        let it = items.get(k - 1).ok_or("bad item number")?;
        todo::mark_done(it, day)?;
        done.push(it.text.clone());
    }
    let mut carried: Vec<String> = items
        .iter()
        .enumerate()
        .filter(|(i, it)| !a.done.contains(&(i + 1)) && matches!(it.place, Place::Line { .. }))
        .map(|(_, it)| it.text.clone())
        .collect();
    carried.extend(a.tomorrow.iter().cloned());
    let tomorrow = day.checked_add_days(Days::new(1)).ok_or("date overflow")?;
    todo::carry(todo_dir, tomorrow, &carried)?;
    Ok((done, carried))
}

/// Handles a night reply. `Ok(None)` means it was not a check-in and nothing was written; the caller files it as usual.
pub fn answer(deps: &Deps, day: NaiveDate, reply: &str) -> Result<Option<String>, String> {
    let items = open_items(&deps.todo_dir, day);
    let (system, user) = answer_prompt(&items, reply, day);
    let parsed = match deps.chat.ask(&system, &user) {
        // No model: keep the words in the log, change nothing else.
        Err(_) => {
            todo::log_checkin(&deps.todo_dir, day, reply, &[], &[])?;
            return Ok(Some("Got it, I logged your check-in. 🐾".to_string()));
        }
        Ok(text) => parse_answer(&text, items.len()),
    };
    let Some(a) = parsed.filter(|a| a.checkin) else { return Ok(None) };
    let (done, carried) = apply(&deps.todo_dir, day, &items, &a)?;
    todo::log_checkin(&deps.todo_dir, day, reply, &done, &carried)?;
    let mut msg = String::new();
    if !done.is_empty() {
        msg.push_str(&format!("Checked off: {}\n", done.join(", ")));
    }
    if !carried.is_empty() {
        msg.push_str(&format!("Carried to tomorrow: {}\n", carried.join(", ")));
    }
    if msg.is_empty() {
        msg.push_str("Logged your check-in. 🐾");
    }
    Ok(Some(msg.trim_end().to_string()))
}
