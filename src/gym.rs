//! #144: the gym session. Pip reads `Gym Plan.md` (never writes it), walks the user through today's session one
//! exercise at a time, and writes each answer as a `pip:gym` block in the day note. Every decision lives here as a
//! function the tests drive; `pip.rs` only applies what `turn` returns.
//! The plan file is not parsed into a schema: two plain settings lines (`start:`, `badminton:`) and headings are read.
use crate::daylog;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Weekday};
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
    // A list item is an exercise even when it is all caps (`- BENCH PRESS`).
    let bullet = t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("+ ")
        || t.starts_with("• ")
        || t.split_once(|c: char| c == '.' || c == ')').is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if bullet {
        return false;
    }
    // Two letters at least, so a bare sets line like `3 X 15` is not a heading.
    !t.is_empty() && t.chars().count() <= 60 && t.chars().filter(|c| c.is_alphabetic()).count() >= 2 && !t.chars().any(|c| c.is_ascii_lowercase())
}

fn heading_text(line: &str) -> String {
    line.trim().trim_start_matches('#').trim().to_lowercase()
}

/// The index of the first heading whose text contains `key`. `section` and `heading_of` both use it.
fn find_heading(lines: &[&str], key: &str) -> Option<usize> {
    let key = key.to_lowercase();
    lines.iter().position(|l| is_heading(l) && heading_text(l).contains(&key))
}

/// The body under the first heading whose text contains `key`, up to the next heading. `None` when absent or empty.
pub fn section(text: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = find_heading(&lines, key)?;
    let body: Vec<&str> = lines[start + 1..].iter().take_while(|l| !is_heading(l)).copied().collect();
    let body = body.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

/// The text of the heading `section` reads for `key`, lowercased and without its `#`s.
fn heading_of(text: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    find_heading(&lines, key).map(|i| heading_text(lines[i]))
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

/// One exercise of a session. `name` is what he types to pick it, and `options` are the name's `A or B` / `A / B`
/// parts. `target` is the bullet's own sets x reps (`2×10/side`, `30 s each side`), `note` is bracketed text with no
/// digit, and `seconds` marks a timed item, whose reply numbers are seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Exercise {
    pub name: String,
    pub options: Vec<String>,
    pub target: Option<String>,
    pub note: Option<String>,
    pub seconds: bool,
}

/// A table row is never an exercise. Every other line, including one with `->`, is read as one.
fn is_table_row(line: &str) -> bool {
    line.starts_with('|')
}

/// The exercise lines of one section body, bullets and numbers stripped.
fn exercise_lines(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in body.lines().map(str::trim) {
        if raw.is_empty() || is_table_row(raw) {
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

/// The `A` in `A × 2` shape: a token that starts with a digit and has `×`, `x` or `X` directly before a digit
/// (`2×10/side`, `3x15`).
fn is_sets_token(token: &str) -> bool {
    token.starts_with(|c: char| c.is_ascii_digit())
        && token.char_indices().any(|(i, c)| matches!(c, '×' | 'x' | 'X') && token[i + c.len_utf8()..].starts_with(|d: char| d.is_ascii_digit()))
}

/// Splits one bullet into (name, target, note). Bracketed text with a digit is the target, bracketed text without one
/// is a note, and the first `NxM`-shaped token starts the target. The name is never empty.
fn split_item(line: &str) -> (String, Option<String>, Option<String>) {
    // Pass 1: each `(`...`)` pair is a group, and the line outside the groups keeps one space where a group was.
    let chars: Vec<char> = line.chars().collect();
    let mut outside = String::new();
    let mut groups: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '(' {
            if let Some(close) = chars[i + 1..].iter().position(|&c| c == ')') {
                groups.push(chars[i + 1..i + 1 + close].iter().collect());
                outside.push(' ');
                i += close + 2;
                continue;
            }
        }
        outside.push(chars[i]);
        i += 1;
    }

    // Pass 2: the first group with a digit is the target; the others are notes.
    let mut paren_target: Option<String> = None;
    let mut notes: Vec<String> = Vec::new();
    for g in groups {
        let t = g.trim().to_string();
        if paren_target.is_none() && t.chars().any(|c| c.is_ascii_digit()) {
            paren_target = Some(t);
        } else if !t.is_empty() {
            notes.push(t);
        }
    }

    // Pass 3: a sets token starts the target, and the tokens from it on are the target.
    let tokens: Vec<&str> = outside.split_whitespace().collect();
    let (name, target) = match tokens.iter().position(|t| is_sets_token(t)) {
        Some(at) => {
            if let Some(p) = paren_target {
                notes.insert(0, p);
            }
            (tokens[..at].join(" "), Some(tokens[at..].join(" ")))
        }
        None => (tokens.join(" "), paren_target),
    };

    // Pass 4: the name loses trailing separators; an empty name falls back to the whole line.
    let name = name.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | ':' | ',')).to_string();
    if name.is_empty() {
        return (line.trim().to_string(), None, None);
    }
    let note = (!notes.is_empty()).then(|| notes.join("; "));
    (name, target, note)
}

/// A timed target: a unit word (`s`, `sec`, `secs`, `second`, `seconds`) or a digit+`s` token (`30s`, `2×20s`).
fn is_seconds(target: &str) -> bool {
    target.to_lowercase().replace('/', " ").split_whitespace().any(|t| {
        matches!(t, "s" | "sec" | "secs" | "second" | "seconds")
            || (t.len() > 1 && t.ends_with('s') && t[..t.len() - 1].ends_with(|c: char| c.is_ascii_digit()))
    })
}

/// One bullet as an exercise. The options are the name's own `A or B` parts.
fn to_exercise(line: String) -> Exercise {
    let (name, target, note) = split_item(&line);
    let mut options: Vec<String> = Vec::new();
    for part in name.replace(" / ", " or ").split(" or ").map(str::trim).filter(|p| !p.is_empty()) {
        if !options.iter().any(|o| o == part) {
            options.push(part.to_string());
        }
    }
    if options.is_empty() {
        options = vec![name.clone()];
    }
    let seconds = target.as_deref().is_some_and(is_seconds);
    Exercise { name, options, target, note, seconds }
}

/// The exercises under `session`'s own heading, in plan order.
fn own(plan_text: &str, session: Session) -> Vec<Exercise> {
    section(plan_text, session.key()).map(|b| exercise_lines(&b).into_iter().map(to_exercise).collect()).unwrap_or_default()
}

/// The core section of a week-3 plan: its items, and the pick range from its heading (`pick 2–3`) when it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct Core {
    pub items: Vec<Exercise>,
    pub range: Option<(usize, usize)>,
}

/// The core for `week`. Empty before week 3. Without a usable pick range, every core item is walked.
pub fn core(plan_text: &str, week: u32) -> Core {
    if week < 3 {
        return Core { items: Vec::new(), range: None };
    }
    let items: Vec<Exercise> = section(plan_text, "any day").map(|b| exercise_lines(&b).into_iter().map(to_exercise).collect()).unwrap_or_default();
    let range = heading_of(plan_text, "any day").and_then(|h| pick_range(&h));
    match range {
        Some((lo, hi)) if items.len() > lo => {
            let hi = hi.min(items.len());
            Core { items, range: Some((lo, hi)) }
        }
        _ => Core { items, range: None },
    }
}

/// The exercises of `session` in `week`: its own, then the core. Every core item, picked or not.
pub fn exercises(plan_text: &str, session: Session, week: u32) -> Vec<Exercise> {
    let mut all = own(plan_text, session);
    all.extend(core(plan_text, week).items);
    all
}

/// The `lo–hi` of a heading's `pick lo–hi` (`–`, `-`, `—` or ` to `), or `pick N` for exactly N. `None` when absent.
pub fn pick_range(heading: &str) -> Option<(usize, usize)> {
    let lower = heading.to_lowercase();
    let rest = lower.split_once("pick")?.1.trim_start();
    let digits = |s: &str| s.chars().take_while(|c| c.is_ascii_digit()).count();
    let n = digits(rest);
    if n == 0 {
        return None;
    }
    let a: usize = rest[..n].parse().ok()?;
    let tail = rest[n..].trim_start();
    let after = tail.strip_prefix(['–', '-', '—']).or_else(|| tail.strip_prefix("to "));
    let b = match after {
        Some(t) => {
            let t = t.trim_start();
            let m = digits(t);
            if m == 0 {
                a
            } else {
                t[..m].parse().ok()?
            }
        }
        None => a,
    };
    (a > 0 && b >= a).then_some((a, b))
}

/// The pick range as the prompt writes it: `2` or `2–3`.
fn range_text((lo, hi): (usize, usize)) -> String {
    if lo == hi {
        format!("{lo}")
    } else {
        format!("{lo}–{hi}")
    }
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

/// An open session: the day it runs on, its session, its week, the DM that started it and when, and the core pick
/// (`None` until he picks; the core items are then walked in the order he typed them, 1-based).
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub day: String,
    pub session: Session,
    pub week: u32,
    pub open_id: u64,
    pub opened: DateTime<FixedOffset>,
    pub core: Option<Vec<usize>>,
}

/// The marker line: `{day} {session key} {week} {open_id} {opened RFC 3339}`, plus ` core=1,3` once a pick is made.
pub fn marker_text(o: &Open) -> String {
    let base = format!("{} {} {} {} {}", o.day, o.session.key(), o.week, o.open_id, o.opened.to_rfc3339());
    match o.core.as_deref() {
        Some(c) if !c.is_empty() => format!("{base} core={}", c.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",")),
        _ => base,
    }
}

fn parse_marker(text: &str) -> Option<Open> {
    let mut tokens: Vec<&str> = text.split_whitespace().collect();
    // A `core=` token is popped first. A list that does not parse leaves the pick unmade, so he is asked again.
    let last = tokens.last().copied().unwrap_or("");
    let core = match last.strip_prefix("core=") {
        Some(list) => {
            tokens.pop();
            list.split(',').map(|n| n.parse::<usize>().ok().filter(|&v| v >= 1)).collect::<Option<Vec<usize>>>()
        }
        None => None,
    };
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
        core,
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

/// Sanity caps on a typed `NxM`: a fat-fingered count must not allocate gigabytes.
const MAX_SETS: u32 = 20;
const MAX_REPS: u32 = 200;

/// Joins sets notation split by spaces (`3 x 15`, `3x 15`, `3 x15`) into one `3x15` token.
fn join_sets(raw: &[&str]) -> Vec<String> {
    let digits = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    let xs = |c: char| matches!(c, 'x' | 'X' | '×');
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if digits(raw[i]) {
            if let Some(rest) = raw.get(i + 1).and_then(|t| t.strip_prefix(xs)) {
                if rest.is_empty() {
                    if let Some(r) = raw.get(i + 2).filter(|t| digits(t)) {
                        out.push(format!("{}x{r}", raw[i]));
                        i += 3;
                        continue;
                    }
                } else if digits(rest) {
                    out.push(format!("{}x{rest}", raw[i]));
                    i += 2;
                    continue;
                }
            }
        }
        if let Some(sets) = raw[i].strip_suffix(xs) {
            if let Some(r) = raw.get(i + 1).filter(|t| digits(t)) {
                if digits(sets) {
                    out.push(format!("{sets}x{r}"));
                    i += 2;
                    continue;
                }
            }
        }
        out.push(raw[i].to_string());
        i += 1;
    }
    out
}

/// Sanity cap on one timed set, in seconds.
const MAX_SECONDS: u32 = 600;

/// Seconds mode: a unit word after a number is dropped (`30 s`), and a unit suffix is cut (`30s`, `30sec`).
fn seconds_tokens(raw: &[&str]) -> Vec<String> {
    const UNITS: [&str; 4] = ["seconds", "secs", "sec", "s"];
    let ends_digit = |t: &str| t.ends_with(|c: char| c.is_ascii_digit());
    let mut out: Vec<String> = Vec::new();
    for &t in raw {
        if UNITS.iter().any(|u| t.eq_ignore_ascii_case(u)) {
            if !out.last().is_some_and(|p| ends_digit(p)) {
                out.push(t.to_string());
            }
            continue;
        }
        let cut = UNITS.iter().find_map(|u| {
            let at = t.len().checked_sub(u.len())?;
            (t.is_char_boundary(at) && t[at..].eq_ignore_ascii_case(u) && ends_digit(&t[..at])).then(|| t[..at].to_string())
        });
        out.push(cut.unwrap_or_else(|| t.to_string()));
    }
    out
}

/// Reads a reply to an exercise prompt: `[what you did] reps...`, `skip`, `stop`.
pub fn parse_reply(text: &str, options: &[String]) -> Reply {
    parse_reply_unit(text, options, false)
}

/// As `parse_reply`. With `seconds`, the numbers are seconds per set (`30 30`, `30s 30s`, `2x30s`), capped at
/// `MAX_SECONDS` instead of `MAX_REPS`.
pub fn parse_reply_unit(text: &str, options: &[String], seconds: bool) -> Reply {
    let body = text.trim().trim_start_matches(['/', '!']).trim().trim_end_matches(['.', '!']).trim();
    let lower = body.to_lowercase();
    match lower.as_str() {
        "stop" | "done" | "end" | "quit" => return Reply::Stop,
        "skip" | "next" => return Reply::Skip,
        _ => {}
    }
    let raw: Vec<&str> = body.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).collect();
    let raw: Vec<String> = if seconds { seconds_tokens(&raw) } else { raw.iter().map(|t| t.to_string()).collect() };
    let raw: Vec<&str> = raw.iter().map(|t| t.as_str()).collect();
    let cap = if seconds { MAX_SECONDS } else { MAX_REPS };
    let joined = join_sets(&raw);
    let tokens: Vec<&str> = joined.iter().map(|s| s.as_str()).collect();
    let is_num = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    let split_nxm = |t: &str| -> Option<(u32, u32)> {
        let (a, b) = t.split_once(|c: char| matches!(c, 'x' | 'X' | '×'))?;
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
            if n > MAX_SETS || m > cap {
                return Reply::Unreadable;
            }
            reps.extend(std::iter::repeat(m).take(n as usize));
        } else if is_num(t) {
            match t.parse() {
                Ok(v) if v <= cap => reps.push(v),
                Ok(_) => return Reply::Unreadable,
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

/// An item's text in a prompt: its options, then its own target or the week's sets x reps, then its note.
fn item_text(ex: &Exercise, week: u32) -> String {
    let target = ex.target.as_deref().unwrap_or(sets_reps(week));
    let mut out = format!("{} · {target}", ex.options.join(" or "));
    if let Some(note) = &ex.note {
        out.push_str(&format!(" · {note}"));
    }
    out
}

/// The prompt for exercise `k` of `n`.
pub fn prompt(ex: &Exercise, k: usize, n: usize, week: u32) -> String {
    let hint = if ex.seconds {
        "Reply with seconds per set (e.g. 30 30 or 30s 30s), or skip / stop."
    } else {
        "Reply with what you did and reps per set (e.g. cable row 15 15 12), or skip / stop."
    };
    format!("{}/{n} · {}\n{hint}", k + 1, item_text(ex, week))
}

/// The exercises this session walks: its own, then the core. Without a pick range that is every core item. With one,
/// it is none until he picks, then the picked items in the order he typed them.
pub fn walk(plan_text: &str, o: &Open) -> Vec<Exercise> {
    let mut list = own(plan_text, o.session);
    let c = core(plan_text, o.week);
    match (c.range, o.core.as_ref()) {
        (None, _) => list.extend(c.items),
        (Some(_), Some(picks)) => list.extend(picks.iter().filter_map(|&i| i.checked_sub(1).and_then(|j| c.items.get(j).cloned()))),
        (Some(_), None) => {}
    }
    list
}

/// The core this session is waiting on a pick for: it has a pick range and no pick is made yet.
fn pending(plan_text: &str, o: &Open) -> Option<Core> {
    let c = core(plan_text, o.week);
    (c.range.is_some() && o.core.is_none()).then_some(c)
}

/// The pick prompt: the range, the numbers to type, and one line per core item.
pub fn pick_prompt(core: &Core, week: u32) -> String {
    let (lo, hi) = core.range.unwrap_or((1, core.items.len()));
    let mut out = format!("Core · pick {} by number (e.g. 1 3), or skip.", range_text((lo, hi)));
    for (i, item) in core.items.iter().enumerate() {
        out.push_str(&format!("\n{}. {}", i + 1, item_text(item, week)));
    }
    out
}

/// What a reply at the pick prompt means.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Chosen(Vec<usize>),
    Skip,
    Stop,
    Bad,
}

/// Reads a pick: 1-based numbers, separated by spaces and/or commas (`and` and `&` are ignored, duplicates keep their
/// first place). The count must be within `lo..=hi`, and each number within `1..=len`. `skip` and `stop` as elsewhere.
pub fn parse_pick(text: &str, len: usize, lo: usize, hi: usize) -> Pick {
    let body = text.trim().trim_start_matches(['/', '!']).trim().trim_end_matches(['.', '!']).trim();
    match body.to_lowercase().as_str() {
        "stop" | "done" | "end" | "quit" => return Pick::Stop,
        "skip" | "next" | "none" | "no" => return Pick::Skip,
        _ => {}
    }
    let mut picks: Vec<usize> = Vec::new();
    for tok in body.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()) {
        let lower = tok.to_lowercase();
        if lower == "and" || lower == "&" {
            continue;
        }
        let Ok(n) = tok.parse::<usize>() else { return Pick::Bad };
        if n == 0 || n > len {
            return Pick::Bad;
        }
        if !picks.contains(&n) {
            picks.push(n);
        }
    }
    if picks.len() < lo || picks.len() > hi {
        return Pick::Bad;
    }
    Pick::Chosen(picks)
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

fn did_line(name: &str, chosen: &str, reps: &[u32], seconds: bool) -> String {
    let unit = if seconds { "seconds" } else { "reps" };
    let reps: Vec<String> = reps.iter().map(|r| r.to_string()).collect();
    if chosen == name {
        format!("{name} · {unit} {}", reps.join(" "))
    } else {
        format!("{name} · {chosen} · {unit} {}", reps.join(" "))
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
            let opened = DateTime::parse_from_rfc3339(received).ok()?;
            let o = Open { day: today_str.clone(), session, week, open_id: msg_id, opened, core: None };
            let list = walk(&plan.text, &o);
            let pend = pending(&plan.text, &o);
            if list.is_empty() && pend.is_none() {
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
            let n = list.len();
            let head = match &pend {
                Some(c) => format!("Week {week} · {} · {n} exercises + core (pick {})", session.label(), range_text(c.range.unwrap_or((1, 1)))),
                None => format!("Week {week} · {} · {n} exercises", session.label()),
            };
            let body = match list.first() {
                Some(ex) => prompt(ex, 0, n, week),
                None => pick_prompt(pend.as_ref()?, week),
            };
            let marker = marker_text(&o);
            return Some(Turn { reply: format!("{head}\n{body}"), day: today_str, tag: String::new(), lines: Vec::new(), marker: Marker::Open(marker) });
        };
        let plan = match load_plan(dir) {
            Ok(p) => p,
            Err(e) => return Some(plan_error(&e, &o.day)),
        };
        let list = walk(&plan.text, o);
        let k = progress(dir, o).len();
        if k < list.len() {
            return Some(Turn {
                reply: prompt(&list[k], k, list.len(), o.week),
                day: o.day.clone(),
                tag: String::new(),
                lines: Vec::new(),
                marker: Marker::Keep,
            });
        }
        if let Some(c) = pending(&plan.text, o) {
            return Some(Turn { reply: pick_prompt(&c, o.week), day: o.day.clone(), tag: String::new(), lines: Vec::new(), marker: Marker::Keep });
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
    let list = walk(&plan.text, &o);
    let k = progress(dir, &o).len();
    let keep = |reply: String| Turn { reply, day: o.day.clone(), tag: String::new(), lines: Vec::new(), marker: Marker::Keep };
    if k >= list.len() {
        // Every own exercise is answered. A pending core waits for its pick; otherwise the session is done.
        let Some(core) = pending(&plan.text, &o) else {
            return Some(finish(dir, &o, Vec::new()));
        };
        let (lo, hi) = core.range.unwrap_or((1, core.items.len()));
        let entries = || -> Vec<Vec<String>> { progress(dir, &o).into_iter().map(|(_, l)| l).collect() };
        return Some(match parse_pick(text, core.items.len(), lo, hi) {
            Pick::Stop => Turn {
                reply: format!("Stopped before core. {}", summary(o.session.label(), &entries())),
                day: o.day.clone(),
                tag: String::new(),
                lines: Vec::new(),
                marker: Marker::Clear,
            },
            Pick::Skip => finish(dir, &o, Vec::new()),
            Pick::Chosen(picks) => {
                let o2 = Open { core: Some(picks), ..o.clone() };
                let l2 = walk(&plan.text, &o2);
                if l2.len() <= k {
                    return Some(finish(dir, &o2, Vec::new()));
                }
                Turn {
                    reply: prompt(&l2[k], k, l2.len(), o.week),
                    day: o.day.clone(),
                    tag: String::new(),
                    lines: Vec::new(),
                    marker: Marker::Open(marker_text(&o2)),
                }
            }
            Pick::Bad => keep(format!("Couldn't read that as a pick. {}", pick_prompt(&core, o.week))),
        });
    }
    let n = list.len();
    let ex = &list[k];
    match parse_reply_unit(text, &ex.options, ex.seconds) {
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
            let line = did_line(&ex.name, &chosen, &reps, ex.seconds);
            Some(advance(dir, &o, &plan.text, k, vec![line]))
        }
        Reply::Ambiguous(ms) => Some(keep(format!("Which one: {}? Reply with the name and reps.", ms.join(" or ")))),
        Reply::Unreadable => Some(keep(format!("Couldn't read that as reps. {}", prompt(ex, k, n, o.week)))),
    }
}

/// The reply after exercise `k` is answered with `lines`: the next prompt, or the finish when it was the last.
fn advance(dir: &Path, o: &Open, plan_text: &str, k: usize, lines: Vec<String>) -> Turn {
    let list = walk(plan_text, o);
    let n = list.len();
    if k + 1 >= n {
        if let Some(core) = pending(plan_text, o) {
            return Turn {
                reply: pick_prompt(&core, o.week),
                day: o.day.clone(),
                tag: format!("gym {} · session {}", o.session.label(), o.open_id),
                lines,
                marker: Marker::Keep,
            };
        }
        finish(dir, o, lines)
    } else {
        Turn {
            reply: prompt(&list[k + 1], k + 1, n, o.week),
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
