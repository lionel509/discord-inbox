//! Stormy's morning brief and night check-in (#135): the slot clock, the facts, the voice pass, and the check-in answer. Facts are built in code; the model only words them and reads the night reply.
use crate::calendar::{self, Cal, Event};
use crate::chat::Chat;
use crate::redact;
use crate::todo::{self, Item};
use crate::vaults::PERSONA;
use crate::weather::{self, Weather};
use chrono::{Days, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use std::fs;
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

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Morning => "morning",
        Slot::Night => "night",
    }
}

fn slot_time(times: &Times, slot: Slot) -> NaiveTime {
    match slot {
        Slot::Morning => times.morning,
        Slot::Night => times.night,
    }
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn minutes(t: NaiveTime) -> i64 {
    (t.hour() * 60 + t.minute()) as i64
}


// A marker file holding its state word (`sent`, `answered`). Written before any DM is sent, so a crash after sending never sends twice.
fn mark(path: &Path, state: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, state.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

pub fn times_from_env() -> Times {
    let pick = |var: &str, default: &str| {
        std::env::var(var)
            .ok()
            .and_then(|s| parse_time(&s))
            .unwrap_or_else(|| parse_time(default).expect("default slot time parses"))
    };
    Times {
        morning: pick("DISCORD_INBOX_MORNING_AT", DEFAULT_MORNING),
        night: pick("DISCORD_INBOX_NIGHT_AT", DEFAULT_NIGHT),
    }
}

pub fn marker_path(inbox: &Path, slot: Slot, day: &str) -> PathBuf {
    inbox.join(format!("stormy-{}-{day}.sent", slot_name(slot)))
}

/// The slot whose window (its time up to GRACE_MIN later) is open now and not yet sent today.
pub fn due(inbox: &Path, times: &Times, now: NaiveDateTime) -> Option<(Slot, String)> {
    let day = ymd(now.date());
    let mins = minutes(now.time());
    [Slot::Morning, Slot::Night]
        .into_iter()
        .find(|&slot| {
            let start = minutes(slot_time(times, slot));
            mins >= start && mins < start + GRACE_MIN && !marker_path(inbox, slot, &day).exists()
        })
        .map(|slot| (slot, day))
}

pub fn stage(inbox: &Path, slot: Slot, day: &str) -> Result<(), String> {
    mark(&marker_path(inbox, slot, day), "sent")
}

/// The most recent night check-in (today's, then yesterday's, so a reply after midnight still counts) that was sent and has no answer yet.
pub fn awaiting(inbox: &Path, now: NaiveDateTime) -> Option<String> {
    // The night marker reads `sent` until the reply is answered, then `answered`.
    let open = |day: &str| fs::read_to_string(marker_path(inbox, Slot::Night, day)).map(|s| s.trim() == "sent").unwrap_or(false);
    let today = ymd(now.date());
    if open(&today) {
        return Some(today);
    }
    let yesterday = ymd(now.date().checked_sub_days(Days::new(1))?);
    open(&yesterday).then_some(yesterday)
}

/// True when the message was received at or after the night marker was staged, so a catch-up replay of an older message is not the reply. Compared in whole seconds, as the stamp is.
pub fn after_send(inbox: &Path, day: &str, received: &str) -> bool {
    let Ok(got) = chrono::DateTime::parse_from_rfc3339(received) else {
        return false;
    };
    let Ok(meta) = fs::metadata(marker_path(inbox, Slot::Night, day)) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let sent: chrono::DateTime<chrono::Utc> = modified.into();
    got.timestamp() >= sent.timestamp()
}

pub fn answered(inbox: &Path, day: &str) -> Result<(), String> {
    mark(&marker_path(inbox, Slot::Night, day), "answered")
}

/// Any open event that is not a lecture or office hour counts as a deadline.
pub fn is_deadline(ev: &Event) -> bool {
    !ev.done() && !ev.calendar.ends_with("Lectures") && !ev.calendar.ends_with("Office Hours")
}

/// An exam calendar, or an exam word as a whole word in the summary ("Example" is not "exam").
pub fn is_exam(ev: &Event) -> bool {
    if ev.calendar.ends_with("Exams") {
        return true;
    }
    ev.summary
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| matches!(w, "exam" | "exams" | "midterm" | "midterms" | "quiz" | "quizzes" | "final" | "finals"))
}

// Events on one day: all-day first, then timed events by clock time.
fn day_sorted<'a>(events: &'a [Event], day: &str) -> Vec<&'a Event> {
    let mut v: Vec<&Event> = events.iter().filter(|e| e.date == day).collect();
    v.sort_by_key(|e| (e.time.is_some(), e.time.clone().unwrap_or_default()));
    v
}

fn event_line(e: &Event) -> String {
    match &e.time {
        None => format!("- all day: {} ({})", e.summary, e.calendar),
        Some(t) => format!("- {t} {} ({})", e.summary, e.calendar),
    }
}

// Deadlines due 1 to AHEAD_DAYS after today.
fn due_soon(events: &[Event], today: NaiveDate) -> Vec<String> {
    let mut v: Vec<(NaiveDate, &Event)> = events
        .iter()
        .filter(|e| is_deadline(e))
        .filter_map(|e| NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok().map(|d| (d, e)))
        .filter(|(d, _)| {
            let gap = (*d - today).num_days();
            gap >= 1 && gap <= AHEAD_DAYS
        })
        .collect();
    v.sort_by_key(|(d, _)| *d);
    v.into_iter().map(|(_, e)| format!("- {} ({}), {}", e.summary, e.calendar, e.date)).collect()
}

// Today's open list: the day file first, then the standing notes.
pub fn check_items(todo_dir: &Path, day: NaiveDate) -> Vec<Item> {
    let mut v = todo::day_items(todo_dir, day);
    v.extend(todo::note_items(todo_dir));
    v
}

/// Where the night DM's numbered list is kept, one item text per line, so the reply is read against the list as it was sent.
pub fn list_path(inbox: &Path, day: &str) -> PathBuf {
    marker_path(inbox, Slot::Night, day).with_extension("list")
}

pub fn save_list(inbox: &Path, day: &str, items: &[Item]) -> Result<(), String> {
    let path = list_path(inbox, day);
    let body: String = items.iter().map(|it| format!("{}\n", it.text.replace(['\r', '\n'], " "))).collect();
    fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load_list(inbox: &Path, day: &str) -> Option<Vec<String>> {
    let body = fs::read_to_string(list_path(inbox, day)).ok()?;
    Some(body.lines().map(str::to_string).collect())
}

pub fn morning_facts(todo_dir: &Path, events: &[Event], weather: Option<&Weather>, today: NaiveDate) -> String {
    let d = ymd(today);
    let mut out = Vec::new();
    match weather {
        Some(w) => {
            out.push(weather::line(w));
            let wear = weather::wear(w);
            out.push(if wear.is_empty() { "Wear: no change".to_string() } else { format!("Wear: {}", wear.join(", ")) });
        }
        None => out.push("Weather: unavailable".to_string()),
    }
    let exam_today = events.iter().any(|e| is_exam(e) && e.date == d && !e.done());
    out.push(if exam_today { "Bring: calculator (exam today)".to_string() } else { "Bring: nothing extra".to_string() });

    let mut today_lines: Vec<String> = day_sorted(events, &d).into_iter().map(event_line).collect();
    today_lines.extend(todo::day_items(todo_dir, today).iter().map(|it| format!("- (todo) {}", it.text)));
    today_lines.extend(todo::note_items(todo_dir).iter().map(|it| format!("- (open) {}", it.text)));
    if today_lines.is_empty() {
        out.push("Today: nothing on the list".to_string());
    } else {
        out.push("Today:".to_string());
        out.extend(today_lines);
    }
    let soon = due_soon(events, today);
    if !soon.is_empty() {
        out.push("Due soon:".to_string());
        out.extend(soon);
    }
    out.join("\n")
}

pub fn night_facts(todo_dir: &Path, events: &[Event], today: NaiveDate) -> String {
    let tomorrow = today.checked_add_days(Days::new(1)).unwrap_or(today);
    let mut out = Vec::new();
    let tm = day_sorted(events, &ymd(tomorrow));
    // The first thing tomorrow is the earliest timed event; an all-day one only when nothing is timed.
    let first = tm
        .iter()
        .find(|e| e.time.is_some())
        .or(tm.first())
        .map(|e| event_line(e).trim_start_matches("- ").to_string())
        .unwrap_or_else(|| "nothing on the calendar".to_string());
    out.push(format!("Tomorrow first: {first}"));
    let soon = due_soon(events, today);
    if !soon.is_empty() {
        out.push("Due soon:".to_string());
        out.extend(soon);
    }
    let items = check_items(todo_dir, today);
    if items.is_empty() {
        out.push("Todo: nothing open".to_string());
    } else {
        out.push("Todo, numbered:".to_string());
        out.extend(items.iter().enumerate().map(|(i, it)| format!("{}. {}", i + 1, it.text)));
    }
    out.join("\n")
}

/// Stormy's wording of the facts. The facts stand when the model is silent or sends a stub.
pub fn voice(chat: &dyn Chat, slot: Slot, facts: &str) -> String {
    let job = match slot {
        Slot::Morning => "Write Lionel's morning brief as a short Discord DM. Keep every fact line with its numbers and names. Add nothing that is not in the facts. Do not mention umbrellas, rain gear or the chance of rain. No markdown headings. Under 1500 characters.",
        Slot::Night => "Write Lionel's night check-in as a short Discord DM. Keep every fact line. End by asking which numbered items he finished and what should carry to tomorrow. No markdown headings. Under 1500 characters.",
    };
    let system = format!("{PERSONA}\n\n{job}");
    match chat.ask(&system, facts) {
        Ok(text) => {
            // The prompt's limits are enforced here: the facts stand when the reply is under a third of the facts, over 1500 characters, or names rain gear the facts do not (a word counted more often in the reply than in the facts).
            let t = text.trim();
            let short = t.chars().count() < facts.chars().count() / 3;
            let long = t.chars().count() > 1500;
            let (lower, lower_facts) = (t.to_lowercase(), facts.to_lowercase());
            let rain = ["umbrella", "rain", "poncho", "\u{2614}", "\u{1f302}"].iter().any(|w| lower.matches(w).count() > lower_facts.matches(w).count());
            if short || long || rain { facts.to_string() } else { text }
        }
        Err(_) => facts.to_string(),
    }
}

pub fn compose(deps: &Deps, slot: Slot, today: NaiveDate) -> String {
    let events = match deps.cal.as_ref() {
        Some(cal) => calendar::list(cal).unwrap_or_else(|e| {
            eprintln!("stormy: calendar list failed: {e}");
            Vec::new()
        }),
        None => Vec::new(),
    };
    let facts = match slot {
        Slot::Morning => {
            let w = weather::fetch();
            morning_facts(&deps.todo_dir, &events, w.as_ref(), today)
        }
        Slot::Night => night_facts(&deps.todo_dir, &events, today),
    };
    let text = voice(deps.chat.as_ref(), slot, &facts);
    redact::sensitive(text.trim()).chars().take(MAX_CHARS).collect()
}

pub fn answer_prompt(texts: &[String], reply: &str, day: NaiveDate) -> (String, String) {
    let system = "You read Lionel's reply to his night check-in. Reply with only a JSON object: {\"checkin\": true or false, \"done\": [numbers of the listed items he finished], \"tomorrow\": [new tasks he names for tomorrow that are not already in the list]}. Set checkin to false when the message is not a check-in. Use only numbers from the list. Do not invent tasks.".to_string();
    let list = if texts.is_empty() {
        "(no open items)".to_string()
    } else {
        texts.iter().enumerate().map(|(i, t)| format!("{}. {t}", i + 1)).collect::<Vec<_>>().join("\n")
    };
    let user = format!("Date: {}\nOpen items:\n{list}\n\nReply:\n{reply}", ymd(day));
    (system, user)
}

/// The model's JSON, with `done` limited to listed numbers 1..=n.
pub fn parse_answer(text: &str, n: usize) -> Option<Answer> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&text[start..=end]).ok()?;
    let checkin = v.get("checkin")?.as_bool()?;
    let mut done: Vec<usize> = v
        .get("done")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.trim().parse::<u64>().ok())))
                .filter(|&x| x >= 1 && x <= n as u64)
                .map(|x| x as usize)
                .collect()
        })
        .unwrap_or_default();
    done.sort();
    done.dedup();
    let tomorrow: Vec<String> = v
        .get("tomorrow")
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .take(20)
                .collect()
        })
        .unwrap_or_default();
    Some(Answer { checkin, done, tomorrow })
}

/// Strikes the listed items the model marked done and carries the rest to tomorrow's file.
pub fn apply(todo_dir: &Path, day: NaiveDate, items: &[Item], a: &Answer) -> Result<(Vec<String>, Vec<String>), String> {
    let mut done = Vec::new();
    for &n in &a.done {
        let Some(item) = n.checked_sub(1).and_then(|i| items.get(i)) else { continue };
        todo::mark_done(item, day)?;
        done.push(item.text.clone());
    }
    // The rest: today's open lines the model did not strike, then the model's own tomorrow items.
    let mut carried: Vec<String> = items
        .iter()
        .enumerate()
        .filter(|(i, it)| matches!(it.place, todo::Place::Line { .. }) && !a.done.contains(&(i + 1)))
        .map(|(_, it)| it.text.clone())
        .collect();
    carried.extend(a.tomorrow.iter().cloned());
    let next = day.checked_add_days(Days::new(1)).ok_or_else(|| "no next day".to_string())?;
    if !carried.is_empty() {
        todo::carry(todo_dir, next, &carried)?;
    }
    Ok((done, carried))
}

// The model's numbers name the list as it was sent. Each becomes the first unused item in the live list with the same text, or nothing when that item is gone.
fn remap_done(done: &[usize], shown: &[String], current: &[String]) -> Vec<usize> {
    let mut used = vec![false; current.len()];
    let mut out = Vec::new();
    for &k in done {
        let Some(text) = k.checked_sub(1).and_then(|i| shown.get(i)) else {
            continue;
        };
        if let Some(j) = (0..current.len()).find(|&j| !used[j] && current[j] == *text) {
            used[j] = true;
            out.push(j + 1);
        }
    }
    out.sort();
    out
}

/// The night reply. `sent` is the list the DM numbered (see `load_list`); without it the live list is used. `Ok(None)` when the message is not a check-in: nothing is written. `Err` when the model is down or its reply is unreadable: nothing is written either, so the caller files the message as usual.
pub fn answer(deps: &Deps, day: NaiveDate, reply: &str, sent: Option<Vec<String>>) -> Result<Option<String>, String> {
    let items = check_items(&deps.todo_dir, day);
    let current: Vec<String> = items.iter().map(|it| it.text.clone()).collect();
    let shown = sent.unwrap_or_else(|| current.clone());
    let (system, user) = answer_prompt(&shown, reply, day);
    let raw = deps.chat.ask(&system, &user).map_err(|_| "check-in model unavailable".to_string())?;
    let mut a = parse_answer(&raw, shown.len()).ok_or_else(|| "check-in model unavailable".to_string())?;
    if !a.checkin {
        return Ok(None);
    }
    a.done = remap_done(&a.done, &shown, &current);
    let (done, carried) = apply(&deps.todo_dir, day, &items, &a)?;
    todo::log_checkin(&deps.todo_dir, day, reply, &done, &carried)?;
    let list = |v: &[String]| if v.is_empty() { "nothing".to_string() } else { v.join(", ") };
    Ok(Some(format!("Checked off: {}. Carried to tomorrow: {}.", list(&done), list(&carried))))
}
