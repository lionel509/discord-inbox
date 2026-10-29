//! #144: the gym session. Pip reads `Gym Plan.md` (never writes it), walks the user through today's session one
//! exercise at a time, and writes each answer as a `pip:gym` block in the day note. Every decision lives here as a
//! function the tests drive; `pip.rs` only applies what `turn` returns.
//! The plan file is not parsed into a schema: two plain settings lines (`start:`, `badminton:`) and headings are read.
use crate::daylog;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Weekday};
use std::collections::HashMap;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

pub const PLAN_FILE: &str = "Gym Plan.md";
/// The open-session marker, beside the day notes.
pub const MARKER: &str = ".gym-open";
/// A session left open this long stops taking replies.
pub const TIMEOUT_HOURS: i64 = 4;

pub fn plan_path(dir: &Path) -> PathBuf {
    dir.join(PLAN_FILE)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Session {
    Day1,
    Day2,
    Push,
    Pull,
    Legs,
}

impl Session {
    pub fn label(self) -> &'static str {
        match self {
            Session::Day1 => "Day 1",
            Session::Day2 => "Day 2",
            Session::Push => "Push",
            Session::Pull => "Pull",
            Session::Legs => "Legs",
        }
    }

    /// The heading text looked up in the plan file.
    pub fn key(self) -> &'static str {
        match self {
            Session::Day1 => "day 1",
            Session::Day2 => "day 2",
            Session::Push => "push",
            Session::Pull => "pull",
            Session::Legs => "leg",
        }
    }
}

/// The session named by a key or a label, in any case (`day 1`, `Push`, `Legs`).
fn session_from_key(s: &str) -> Option<Session> {
    match s.trim().to_lowercase().as_str() {
        "day 1" | "day1" => Some(Session::Day1),
        "day 2" | "day2" => Some(Session::Day2),
        "push" => Some(Session::Push),
        "pull" => Some(Session::Pull),
        "leg" | "legs" => Some(Session::Legs),
        _ => None,
    }
}

/// The two settings lines of the plan file: `start: YYYY-MM-DD` and `badminton: Tue Thu`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub start: Option<NaiveDate>,
    pub badminton: Vec<Weekday>,
}

fn weekday(tok: &str) -> Option<Weekday> {
    let t: String = tok.chars().take(3).collect();
    match t.as_str() {
        "mon" => Some(Weekday::Mon),
        "tue" => Some(Weekday::Tue),
        "wed" => Some(Weekday::Wed),
        "thu" => Some(Weekday::Thu),
        "fri" => Some(Weekday::Fri),
        "sat" => Some(Weekday::Sat),
        "sun" => Some(Weekday::Sun),
        _ => None,
    }
}

pub fn settings_from(plan_text: &str) -> Settings {
    let mut s = Settings::default();
    for line in plan_text.lines() {
        let t = line.trim().to_lowercase();
        if let Some(v) = t.strip_prefix("start:") {
            s.start = NaiveDate::parse_from_str(v.trim(), "%Y-%m-%d").ok();
        } else if let Some(v) = t.strip_prefix("badminton:") {
            s.badminton = v.split(|c: char| c.is_whitespace() || c == ',').filter_map(weekday).collect();
        }
    }
    s
}

pub struct Plan {
    pub text: String,
    pub settings: Settings,
}

pub fn load_plan(dir: &Path) -> Result<Plan, String> {
    let path = plan_path(dir);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let settings = settings_from(&text);
            Ok(Plan { text, settings })
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Err(format!("no plan file at {}", path.display())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn is_heading(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with('#') {
        return true;
    }
    !t.is_empty() && t.chars().count() <= 60 && t.chars().any(|c| c.is_alphabetic()) && !t.chars().any(|c| c.is_ascii_lowercase())
}

fn heading_text(line: &str) -> String {
    line.trim().trim_start_matches('#').trim().to_lowercase()
}

/// The body under the first heading whose text contains `key`, up to the next heading. `None` when absent or empty.
pub fn section(text: &str, key: &str) -> Option<String> {
    let key = key.to_lowercase();
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| is_heading(l) && heading_text(l).contains(&key))?;
    let body: Vec<&str> = lines[start + 1..].iter().take_while(|l| !is_heading(l)).copied().collect();
    let body = body.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

pub fn monday(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// Weeks since the start's Monday, counted from 1. A day before the start is week 1.
pub fn week_number(start: NaiveDate, day: NaiveDate) -> u32 {
    let weeks = (monday(day) - monday(start)).num_days() / 7;
    (weeks + 1).max(1) as u32
}

pub fn sessions_per_week(week: u32) -> u32 {
    if week <= 2 {
        2
    } else {
        3
    }
}

pub fn sets_reps(week: u32) -> &'static str {
    match week {
        0 | 1 => "1×15",
        2 => "2×15",
        _ => "2–3×15–20",
    }
}

/// Weeks 1-2 alternate Day 1 and Day 2. Week 3 on rotates Push, Pull, Legs. Legs never falls the day before badminton.
pub fn next_session(week: u32, last: Option<Session>, tomorrow_badminton: bool) -> Session {
    let pick = if week <= 2 {
        match last {
            Some(Session::Day1) => Session::Day2,
            _ => Session::Day1,
        }
    } else {
        match last {
            Some(Session::Push) => Session::Pull,
            Some(Session::Pull) => Session::Legs,
            _ => Session::Push,
        }
    };
    if pick == Session::Legs && tomorrow_badminton {
        Session::Push
    } else {
        pick
    }
}

/// One exercise of a session. `options[0]` is the plan's own entry; the rest are the `A or B` parts and swap targets.
#[derive(Clone, Debug, PartialEq)]
pub struct Exercise {
    pub name: String,
    pub options: Vec<String>,
}

/// Lowercased source → target pairs from the whole plan: `A → B`, `A -> B`, and two-cell table rows `| A | B |`.
fn swap_table(plan_text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in plan_text.lines().map(str::trim) {
        if line.starts_with('|') {
            let cells: Vec<&str> = line.split('|').map(str::trim).filter(|c| !c.is_empty()).collect();
            if cells.len() == 2 && !cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':'))) {
                map.insert(cells[0].to_lowercase(), cells[1].to_string());
            }
        } else if let Some((a, b)) = line.split_once('→').or_else(|| line.split_once("->")) {
            if !a.trim().is_empty() && !b.trim().is_empty() {
                map.insert(a.trim().to_lowercase(), b.trim().to_string());
            }
        }
    }
    map
}

fn is_swap_or_table(line: &str) -> bool {
    line.starts_with('|') || line.contains('→') || line.contains("->")
}

/// The exercise lines of one section body, bullets and numbers stripped.
fn exercise_lines(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in body.lines().map(str::trim) {
        if raw.is_empty() || is_swap_or_table(raw) {
            continue;
        }
        let lower = raw.to_lowercase();
        if lower.starts_with("start:") || lower.starts_with("badminton:") {
            continue;
        }
        let stripped = raw
            .strip_prefix("- ")
            .or_else(|| raw.strip_prefix("* "))
            .or_else(|| raw.strip_prefix("• "))
            .unwrap_or_else(|| {
                let digits = raw.chars().take_while(|c| c.is_ascii_digit()).count();
                let rest = &raw[digits..];
                if digits > 0 && (rest.starts_with('.') || rest.starts_with(')')) {
                    &rest[1..]
                } else {
                    raw
                }
            })
            .trim();
        if !stripped.is_empty() {
            out.push(stripped.to_string());
        }
    }
    out
}

/// The exercises of `session` in `week`, from its heading in the plan. Week 3 on adds the `Any day` section.
pub fn exercises(plan_text: &str, session: Session, week: u32) -> Vec<Exercise> {
    let swaps = swap_table(plan_text);
    let mut names = section(plan_text, session.key()).map(|b| exercise_lines(&b)).unwrap_or_default();
    if week >= 3 {
        names.extend(section(plan_text, "any day").map(|b| exercise_lines(&b)).unwrap_or_default());
    }
    names
        .into_iter()
        .map(|name| {
            let mut options: Vec<String> = Vec::new();
            for part in name.replace(" / ", " or ").split(" or ").map(str::trim).filter(|p| !p.is_empty()) {
                if !options.iter().any(|o| o == part) {
                    options.push(part.to_string());
                }
                if let Some(target) = swaps.get(&part.to_lowercase()) {
                    if !options.iter().any(|o| o == target) {
                        options.push(target.clone());
                    }
                }
            }
            Exercise { name, options }
        })
        .collect()
}

/// The `pip:gym` blocks of one session's day. Each is `(msg_id, heading, lines)`.
pub fn blocks_of(dir: &Path, day: &str) -> Vec<(u64, String, Vec<String>)> {
    daylog::gym_blocks(dir, day)
}

/// The session a block's heading names (`... · gym Day 1 · session 7`).
fn heading_session(heading: &str) -> Option<Session> {
    heading.split(" · ").find_map(|p| p.strip_prefix("gym ")).and_then(session_from_key)
}

/// The first day with gym blocks, from the day-note file names.
fn first_gym_day(dir: &Path) -> Option<NaiveDate> {
    let mut days: Vec<NaiveDate> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".md").and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        })
        .collect();
    days.sort();
    days.into_iter().find(|d| !blocks_of(dir, &d.format("%Y-%m-%d").to_string()).is_empty())
}

pub fn start_date(dir: &Path, s: &Settings, today: NaiveDate) -> NaiveDate {
    s.start.or_else(|| first_gym_day(dir)).unwrap_or(today)
}

/// The session of the most recent gym block in the 14 days up to and including `today`.
pub fn last_session(dir: &Path, today: NaiveDate) -> Option<Session> {
    (0..14).map(|i| today - Duration::days(i)).find_map(|d| {
        blocks_of(dir, &d.format("%Y-%m-%d").to_string())
            .last()
            .and_then(|(_, heading, _)| heading_session(heading))
    })
}

/// An open session: the day it runs on, its session, its week, the DM that started it and when.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub day: String,
    pub session: Session,
    pub week: u32,
    pub open_id: u64,
    pub opened: DateTime<FixedOffset>,
}

/// The marker line: `{day} {session key} {week} {open_id} {opened RFC 3339}`.
pub fn marker_text(o: &Open) -> String {
    format!("{} {} {} {} {}", o.day, o.session.key(), o.week, o.open_id, o.opened.to_rfc3339())
}

fn parse_marker(text: &str) -> Option<Open> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() < 5 {
        return None;
    }
    let n = tokens.len();
    Some(Open {
        day: tokens[0].to_string(),
        session: session_from_key(&tokens[1..n - 3].join(" "))?,
        week: tokens[n - 3].parse().ok()?,
        open_id: tokens[n - 2].parse().ok()?,
        opened: DateTime::parse_from_rfc3339(tokens[n - 1]).ok()?,
    })
}

pub fn write_marker(dir: &Path, text: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(MARKER);
    let tmp = dir.join(format!("{MARKER}.tmp"));
    fs::write(&tmp, format!("{text}\n"))?;
    fs::rename(&tmp, path)
}

pub fn clear_marker(dir: &Path) -> io::Result<()> {
    match fs::remove_file(dir.join(MARKER)) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The open session, if the marker parses and `received` is within the timeout of its opening. A reply older than
/// the opening (a replay) or past the timeout is not a reply. The marker is left in place.
pub fn read_open(dir: &Path, received: &str) -> Option<Open> {
    let text = fs::read_to_string(dir.join(MARKER)).ok()?;
    let open = parse_marker(&text)?;
    let at = DateTime::parse_from_rfc3339(received).ok()?;
    let elapsed = at - open.opened;
    (elapsed >= Duration::zero() && elapsed <= Duration::hours(TIMEOUT_HOURS)).then_some(open)
}

/// The blocks of this session so far, as `(heading, lines)`. Its length is the index of the current exercise.
pub fn progress(dir: &Path, o: &Open) -> Vec<(String, Vec<String>)> {
    let suffix = format!("session {}", o.open_id);
    blocks_of(dir, &o.day)
        .into_iter()
        .filter(|(_, heading, _)| heading.ends_with(&suffix))
        .map(|(_, heading, lines)| (heading, lines))
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Did { option: Option<String>, reps: Vec<u32> },
    Skip,
    Stop,
    Ambiguous(Vec<String>),
    Unreadable,
}

/// Reads a reply to an exercise prompt: `[what you did] reps...`, `skip`, `stop`.
pub fn parse_reply(text: &str, options: &[String]) -> Reply {
    let body = text.trim().trim_start_matches(['/', '!']).trim();
    let lower = body.to_lowercase();
    match lower.as_str() {
        "stop" | "done" | "end" | "quit" => return Reply::Stop,
        "skip" | "next" => return Reply::Skip,
        _ => {}
    }
    let tokens: Vec<&str> = body.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).collect();
    let is_num = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    let split_nxm = |t: &str| -> Option<(u32, u32)> {
        let (a, b) = t.split_once('x').or_else(|| t.split_once('×'))?;
        if is_num(a) && is_num(b) {
            Some((a.parse().ok()?, b.parse().ok()?))
        } else {
            None
        }
    };
    let first_num = tokens.iter().position(|t| is_num(t) || split_nxm(t).is_some());
    let Some(first_num) = first_num else { return Reply::Unreadable };
    let pick_tokens = &tokens[..first_num];
    let mut reps: Vec<u32> = Vec::new();
    for t in &tokens[first_num..] {
        if let Some((n, m)) = split_nxm(t) {
            reps.extend(std::iter::repeat(m).take(n as usize));
        } else if is_num(t) {
            match t.parse() {
                Ok(v) => reps.push(v),
                Err(_) => return Reply::Unreadable,
            }
        } else {
            return Reply::Unreadable;
        }
    }
    if reps.is_empty() {
        return Reply::Unreadable;
    }
    let pick = pick_tokens.join(" ");
    if pick.is_empty() {
        return Reply::Did { option: None, reps };
    }
    let pick_lower = pick.to_lowercase();
    if let Some(exact) = options.iter().find(|o| o.to_lowercase() == pick_lower) {
        return Reply::Did { option: Some(exact.clone()), reps };
    }
    let partial: Vec<String> = options.iter().filter(|o| o.to_lowercase().contains(&pick_lower)).cloned().collect();
    match partial.len() {
        0 => Reply::Did { option: Some(pick), reps },
        1 => Reply::Did { option: partial.into_iter().next(), reps },
        _ => Reply::Ambiguous(partial),
    }
}

/// The prompt for exercise `k` of `n`.
pub fn prompt(ex: &Exercise, k: usize, n: usize, week: u32) -> String {
    format!(
        "{}/{n} · {} · {}\nReply with what you did and reps per set (e.g. cable row 15 15 12), or skip / stop.",
        k + 1,
        ex.options.join(" or "),
        sets_reps(week)
    )
}

#[derive(Clone, Debug, PartialEq)]
pub enum Marker {
    Keep,
    Open(String),
    Clear,
}

/// What a message does to the gym session. `lines` empty means write no block.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub reply: String,
    pub day: String,
    pub tag: String,
    pub lines: Vec<String>,
    pub marker: Marker,
}

fn plan_error(e: &str, day: &str) -> Turn {
    Turn { reply: format!("Can't read the gym plan: {e}"), day: day.to_string(), tag: String::new(), lines: Vec::new(), marker: Marker::Keep }
}

/// The finish reply: the session's label and every block's line, the current reply's lines included.
fn summary(label: &str, entries: &[Vec<String>]) -> String {
    let skipped = entries.iter().flatten().filter(|l| l.starts_with("skipped:")).count();
    let done = entries.iter().flatten().count() - skipped;
    let mut out = format!("{label} done · {done} logged, {skipped} skipped");
    for line in entries.iter().flatten() {
        out.push_str(&format!("\n- {line}"));
    }
    out
}

fn did_line(name: &str, chosen: &str, reps: &[u32]) -> String {
    let reps: Vec<String> = reps.iter().map(|r| r.to_string()).collect();
    if chosen == name {
        format!("{name} · reps {}", reps.join(" "))
    } else {
        format!("{name} · {chosen} · reps {}", reps.join(" "))
    }
}

/// Handles one DM for the gym session. `None` means the DM is not for the gym (a meal, a note, or a command).
pub fn turn(dir: &Path, received: &str, text: &str, msg_id: u64) -> Option<Turn> {
    let t = text.trim().trim_start_matches(['/', '!']).trim().to_lowercase();
    let today_str = daylog::day_key(received).to_string();
    let open = read_open(dir, received);

    if t == "gym" {
        let Some(o) = open.as_ref() else {
            let plan = match load_plan(dir) {
                Ok(p) => p,
                Err(e) => return Some(plan_error(&e, &today_str)),
            };
            let today = NaiveDate::parse_from_str(&today_str, "%Y-%m-%d").ok()?;
            let s = &plan.settings;
            let week = week_number(start_date(dir, s, today), today);
            let tomorrow_badminton = s.badminton.contains(&(today + Duration::days(1)).weekday());
            let session = next_session(week, last_session(dir, today), tomorrow_badminton);
            let exs = exercises(&plan.text, session, week);
            if exs.is_empty() {
                return Some(Turn {
                    reply: format!(
                        "The plan has no exercise lines under a '{}' heading (headings I look for: Day 1, Day 2, Push, Pull, Legs, Any day).",
                        session.label()
                    ),
                    day: today_str,
                    tag: String::new(),
                    lines: Vec::new(),
                    marker: Marker::Keep,
                });
            }
            let n = exs.len();
            let reply = format!("Week {week} · {} · {n} exercises\n{}", session.label(), prompt(&exs[0], 0, n, week));
            let opened = DateTime::parse_from_rfc3339(received).ok()?;
            let marker = marker_text(&Open { day: today_str.clone(), session, week, open_id: msg_id, opened });
            return Some(Turn { reply, day: today_str, tag: String::new(), lines: Vec::new(), marker: Marker::Open(marker) });
        };
        let plan = match load_plan(dir) {
            Ok(p) => p,
            Err(e) => return Some(plan_error(&e, &o.day)),
        };
        let exs = exercises(&plan.text, o.session, o.week);
        let k = progress(dir, o).len();
        if k < exs.len() {
            return Some(Turn {
                reply: prompt(&exs[k], k, exs.len(), o.week),
                day: o.day.clone(),
                tag: String::new(),
                lines: Vec::new(),
                marker: Marker::Keep,
            });
        }
        return Some(finish(dir, o, Vec::new()));
    }

    let o = open?;
    if ["today", "day", "week", "weekly", "help", "?", "commands"].contains(&t.as_str()) {
        return None;
    }
    if t == "undo" || t.starts_with("fix") {
        return Some(Turn {
            reply: "undo and fix are for meals. In a gym session send the exercise and reps, skip, or stop.".to_string(),
            day: o.day.clone(),
            tag: String::new(),
            lines: Vec::new(),
            marker: Marker::Keep,
        });
    }

    let plan = match load_plan(dir) {
        Ok(p) => p,
        Err(e) => return Some(plan_error(&e, &o.day)),
    };
    let exs = exercises(&plan.text, o.session, o.week);
    let k = progress(dir, &o).len();
    let n = exs.len();
    if k >= n {
        return Some(finish(dir, &o, Vec::new()));
    }
    let ex = &exs[k];
    let keep = |reply: String| Turn { reply, day: o.day.clone(), tag: String::new(), lines: Vec::new(), marker: Marker::Keep };
    match parse_reply(text, &ex.options) {
        Reply::Stop => {
            let entries: Vec<Vec<String>> = progress(dir, &o).into_iter().map(|(_, l)| l).collect();
            Some(Turn {
                reply: format!("Stopped at {k}/{n}. {}", summary(o.session.label(), &entries)),
                day: o.day.clone(),
                tag: String::new(),
                lines: Vec::new(),
                marker: Marker::Clear,
            })
        }
        Reply::Skip => {
            let line = format!("skipped: {}", ex.name);
            Some(advance(dir, &o, &plan.text, k, vec![line]))
        }
        Reply::Did { option, reps } => {
            let chosen = option.unwrap_or_else(|| ex.options[0].clone());
            let line = did_line(&ex.name, &chosen, &reps);
            Some(advance(dir, &o, &plan.text, k, vec![line]))
        }
        Reply::Ambiguous(ms) => Some(keep(format!("Which one: {}? Reply with the name and reps.", ms.join(" or ")))),
        Reply::Unreadable => Some(keep(format!("Couldn't read that as reps. {}", prompt(ex, k, n, o.week)))),
    }
}

/// The reply after exercise `k` is answered with `lines`: the next prompt, or the finish when it was the last.
fn advance(dir: &Path, o: &Open, plan_text: &str, k: usize, lines: Vec<String>) -> Turn {
    let exs = exercises(plan_text, o.session, o.week);
    let n = exs.len();
    if k + 1 >= n {
        finish(dir, o, lines)
    } else {
        Turn {
            reply: prompt(&exs[k + 1], k + 1, n, o.week),
            day: o.day.clone(),
            tag: format!("gym {} · session {}", o.session.label(), o.open_id),
            lines,
            marker: Marker::Keep,
        }
    }
}

/// Ends the session: the summary counts the blocks already written plus `lines`, which this turn writes.
fn finish(dir: &Path, o: &Open, lines: Vec<String>) -> Turn {
    let mut entries: Vec<Vec<String>> = progress(dir, o).into_iter().map(|(_, l)| l).collect();
    if !lines.is_empty() {
        entries.push(lines.clone());
    }
    Turn {
        reply: summary(o.session.label(), &entries),
        day: o.day.clone(),
        tag: format!("gym {} · session {}", o.session.label(), o.open_id),
        lines,
        marker: Marker::Clear,
    }
}

/// One line for a day card: `Day 1 · 2 done, 1 skipped`, or `none`.
pub fn day_line(dir: &Path, day: &str) -> String {
    let blocks = blocks_of(dir, day);
    let Some((_, heading, _)) = blocks.first() else { return "none".to_string() };
    let label = heading.split(" · ").find_map(|p| p.strip_prefix("gym ")).unwrap_or("gym").to_string();
    let skipped = blocks.iter().filter(|(_, _, l)| l.first().is_some_and(|x| x.starts_with("skipped:"))).count();
    format!("{label} · {} done, {skipped} skipped", blocks.len() - skipped)
}
