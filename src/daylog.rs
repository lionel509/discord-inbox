//! Day notes (U3). Pip owns only the lines between `<!-- pip:totals -->` and `<!-- /pip:totals -->`, which it rewrites, and the meal blocks between `<!-- pip:meal N -->` and `<!-- /pip:meal N -->`, which it writes once. Every other byte is the user's: a write splices the totals interior and appends at the end, and never parses front matter or titles.
use crate::dump;
use crate::nutrition::Macros;
use std::collections::HashSet;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// A message that is not a meal (a chat line, a photo): logged so replays skip it, never counted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoteMsg {
    pub msg_id: u64,
    pub time: String,
    pub text: String,
    pub photos: Vec<String>,
}

/// One live meal as read back from the note.
#[derive(Clone, Debug, PartialEq)]
pub struct MealRow {
    pub msg_id: u64,
    pub time: String,
    pub kcal: f64,
    pub lines: Vec<String>,
}

const MEALS_HEADING: &str = "## Meals";
const LEDGER: &str = ".pip-commands";
const DEFAULT_LOW_KCAL: u32 = 1800;
const TOTALS_OPEN: &str = "<!-- pip:totals -->";
const TOTALS_CLOSE: &str = "<!-- /pip:totals -->";
const DAMAGED_LINE: &str = "- ⚠️ totals not updated: the totals marker lines were edited by hand\n";

/// Local date of a message: the first ten characters of its RFC 3339 timestamp.
pub fn day_key(received: &str) -> &str {
    received.get(..10).unwrap_or(received)
}

pub fn note_path(dir: &Path, day: &str) -> PathBuf {
    dir.join(format!("{day}.md"))
}

fn read_note(dir: &Path, day: &str) -> io::Result<Option<String>> {
    match fs::read_to_string(note_path(dir, day)) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn meal_open(id: u64) -> String {
    format!("<!-- pip:meal {id} -->")
}

fn meal_close(id: u64) -> String {
    format!("<!-- /pip:meal {id} -->")
}

/// The id on a marker line `<!-- {prefix}N -->` with N in ASCII digits.
fn marker_id(line: &str, prefix: &str) -> Option<u64> {
    let s = line.trim().strip_prefix(prefix)?.strip_suffix(" -->")?;
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The id on a meal open marker line: `<!-- pip:meal N -->`.
fn meal_open_id(line: &str) -> Option<u64> {
    marker_id(line, "<!-- pip:meal ")
}

/// The id on a meal close marker line: `<!-- /pip:meal N -->`.
fn meal_close_id(line: &str) -> Option<u64> {
    marker_id(line, "<!-- /pip:meal ")
}

/// The id on either marker of a meal. A meal whose open marker he deleted is still logged by its close.
fn meal_id(line: &str) -> Option<u64> {
    meal_open_id(line).or_else(|| meal_close_id(line))
}

/// The id on any Pip marker that records a message: a meal, a note, or an undone meal.
fn any_id(line: &str) -> Option<u64> {
    meal_id(line)
        .or_else(|| marker_id(line, "<!-- pip:note "))
        .or_else(|| marker_id(line, "<!-- /pip:note "))
        .or_else(|| marker_id(line, "<!-- pip:undone "))
        .or_else(|| marker_id(line, "<!-- /pip:undone "))
}

pub fn logged(dir: &Path, day: &str, msg_id: u64) -> bool {
    matches!(read_note(dir, day), Ok(Some(t)) if t.lines().any(|l| any_id(l) == Some(msg_id)))
}

pub fn has_meal(dir: &Path, day: &str) -> bool {
    matches!(read_note(dir, day), Ok(Some(t)) if t.lines().any(|l| meal_id(l).is_some()))
}

/// Collapses whitespace and newlines so free text can never break the markdown layout.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn macros_line(m: &Macros, veg: bool) -> String {
    format!(
        "- macros: {} kcal · P {} g · C {} g · F {} g · veg {}",
        m.kcal.round() as i64,
        m.protein.round() as i64,
        m.carbs.round() as i64,
        m.fat.round() as i64,
        yes_no(veg)
    )
}

/// One meal's region. `damaged` adds a warning line when the totals markers could not be spliced.
fn meal_block(meal: &Meal, damaged: bool) -> String {
    let mut s = format!("{}\n", meal_open(meal.msg_id));
    s.push_str(&format!("### {} · msg {}\n", one_line(&meal.time), meal.msg_id));
    for l in &meal.lines {
        let l = l.trim_end();
        let l = if l.starts_with("- ") { l.to_string() } else { format!("- {l}") };
        s.push_str(&l);
        s.push('\n');
    }
    s.push_str(&macros_line(&meal.macros, meal.veg));
    s.push('\n');
    if let Some(r) = &meal.recipe {
        s.push_str(&format!("- recipe: {}\n", one_line(r)));
    }
    if !meal.unknown.is_empty() {
        let u: Vec<String> = meal.unknown.iter().map(|u| one_line(u)).collect();
        s.push_str(&format!("- unknown: {}\n", u.join(", ")));
    }
    if let Some(n) = &meal.note {
        s.push_str(&format!("- note: {}\n", one_line(n)));
    }
    for p in &meal.photos {
        s.push_str(&format!("- photo: ![[{p}]]\n"));
    }
    if damaged {
        s.push_str(DAMAGED_LINE);
    }
    s.push_str(&meal_close(meal.msg_id));
    s.push('\n');
    s
}

/// Parses the text after `- macros: ` into macros and the veg flag.
fn parse_macros(rest: &str) -> Option<(Macros, bool)> {
    let t: Vec<&str> = rest.split_whitespace().collect();
    let after = |label: &str| -> Option<f64> {
        let i = t.iter().position(|x| *x == label)?;
        t.get(i + 1)?.parse().ok()
    };
    let kcal: f64 = t.first()?.parse().ok()?;
    let veg = t.iter().position(|x| *x == "veg").and_then(|i| t.get(i + 1)) == Some(&"yes");
    let m = Macros { kcal, protein: after("P")?, carbs: after("C")?, fat: after("F")? };
    Some((m, veg))
}

/// Sums the first `- macros:` line of each distinct meal region and derives the flags. Only the marker
/// lines are structure, so a typed heading or macros line outside a meal region is never counted.
fn summarize(text: &str, low_kcal: u32) -> Totals {
    let mut sum = Macros::default();
    let (mut meals, mut veg) = (0usize, false);
    let mut seen = HashSet::new();
    let (mut current, mut counted) = (false, false);
    for line in text.lines() {
        let t = line.trim();
        if let Some(id) = meal_open_id(t) {
            // A repeated open of an id already seen changes nothing.
            if seen.insert(id) {
                current = true;
                counted = false;
                meals += 1;
            }
            continue;
        }
        if t.starts_with("<!-- pip:") || t.starts_with("<!-- /pip:") {
            current = false;
            continue;
        }
        if !(current && !counted) {
            continue;
        }
        let Some(rest) = t.strip_prefix("- macros: ") else { continue };
        let Some((m, v)) = parse_macros(rest) else { continue };
        sum.kcal += m.kcal;
        sum.protein += m.protein;
        sum.carbs += m.carbs;
        sum.fat += m.fat;
        veg |= v;
        counted = true;
    }
    let flags = flags_for(meals, veg, sum.kcal, low_kcal);
    Totals { macros: sum, meals, veg, flags }
}

/// The flags for a day: `no-veg` once a meal is logged without veg, `low-kcal` below the floor.
fn flags_for(meals: usize, veg: bool, kcal: f64, low_kcal: u32) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if meals >= 1 && !veg {
        flags.push("no-veg");
    }
    if kcal < low_kcal as f64 {
        flags.push("low-kcal");
    }
    flags
}

/// True when `s` is exactly what `totals_body` renders for the totals its own first line states. Only
/// then may Pip rewrite the interior; any hand edit makes it false.
fn body_is_pip(s: &str, low_kcal: u32) -> bool {
    if s.is_empty() {
        return true;
    }
    let Some(first) = s.lines().next() else { return false };
    let tok: Vec<&str> = first.split_whitespace().collect();
    if tok.len() != 21 {
        return false;
    }
    let (Ok(k), Ok(p), Ok(c), Ok(f), Ok(n)) = (
        tok[1].parse::<i64>(),
        tok[5].parse::<i64>(),
        tok[9].parse::<i64>(),
        tok[13].parse::<i64>(),
        tok[16].parse::<usize>(),
    ) else {
        return false;
    };
    let veg = tok[20] == "yes";
    let macros = Macros { kcal: k as f64, protein: p as f64, carbs: c as f64, fat: f as f64 };
    let t = Totals { macros, meals: n, veg, flags: flags_for(n, veg, k as f64, low_kcal) };
    s == totals_body(&t, low_kcal)
}

/// The two lines Pip rewrites inside the totals markers.
fn totals_body(t: &Totals, low_kcal: u32) -> String {
    let k = t.macros.kcal as i64;
    let p = t.macros.protein as i64;
    let c = t.macros.carbs as i64;
    let f = t.macros.fat as i64;
    let n = t.meals;
    let s = if n == 1 { "" } else { "s" };
    let v = yes_no(t.veg);
    let flags_md = if t.flags.is_empty() {
        "none".to_string()
    } else {
        let parts: Vec<String> = t
            .flags
            .iter()
            .map(|f| match *f {
                "no-veg" => "⚠️ no veg".to_string(),
                _ => format!("⚠️ low kcal ({k} < {low_kcal})"),
            })
            .collect();
        parts.join(" · ")
    };
    format!("**Total:** {k} kcal · P {p} g · C {c} g · F {f} g · {n} meal{s} · veg {v}\n**Flags:** {flags_md}\n")
}

/// Where the totals interior is, or why there is none. `Ok(a, b)` is the byte range from after the open
/// marker's line to the start of the close marker's line, and only when the note has exactly one open
/// and one close in that order and the bytes between them are Pip's own rendering.
enum Span {
    /// The note has no totals marker line at all.
    Fresh,
    Ok(usize, usize),
    /// Some marker line is missing, doubled or out of order, or the interior is not Pip's: Pip deletes
    /// nothing and only appends.
    Damaged,
}

fn totals_span(text: &str, low_kcal: u32) -> Span {
    let (mut pos, mut opens, mut closes) = (0usize, Vec::new(), Vec::new());
    for line in text.split_inclusive('\n') {
        if line.trim() == TOTALS_OPEN {
            opens.push(pos + line.len());
        } else if line.trim() == TOTALS_CLOSE {
            closes.push(pos);
        }
        pos += line.len();
    }
    match (opens.as_slice(), closes.as_slice()) {
        ([], []) => Span::Fresh,
        ([a], [b]) if a <= b && body_is_pip(&text[*a..*b], low_kcal) => Span::Ok(*a, *b),
        _ => Span::Damaged,
    }
}

/// Ends `text` with a blank line, so an appended block starts after one.
fn separate(text: &mut String) {
    if text.is_empty() {
        return;
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.ends_with("\n\n") {
        text.push('\n');
    }
}

/// Appends one meal to the day's note. A message id already in the note writes nothing.
pub fn write_meal(dir: &Path, day: &str, meal: &Meal, low_kcal: u32) -> io::Result<Written> {
    let _guard = dump::LOCK.lock().unwrap_or_else(|p| p.into_inner());
    fs::create_dir_all(dir)?;
    let existing = read_note(dir, day)?.unwrap_or_default();
    if existing.lines().any(|l| any_id(l) == Some(meal.msg_id)) {
        return Ok(Written::Already);
    }
    let mut text = existing;
    // The byte range of the totals interior to rewrite after the meal is appended. Everything after it
    // is appended, so the offsets stay valid.
    let mut span = None;
    if text.is_empty() {
        text = format!("# {day}\n\n{TOTALS_OPEN}\n");
        let a = text.len();
        text.push_str(&format!("{TOTALS_CLOSE}\n\n{MEALS_HEADING}\n\n"));
        span = Some((a, a));
    } else {
        match totals_span(&text, low_kcal) {
            Span::Ok(a, b) => span = Some((a, b)),
            Span::Fresh => {
                separate(&mut text);
                text.push_str(&format!("{TOTALS_OPEN}\n"));
                let a = text.len();
                text.push_str(&format!("{TOTALS_CLOSE}\n"));
                span = Some((a, a));
            }
            Span::Damaged => {}
        }
    }
    separate(&mut text);
    text.push_str(&meal_block(meal, span.is_none()));
    if let Some((a, b)) = span {
        let body = totals_body(&summarize(&text, low_kcal), low_kcal);
        text.replace_range(a..b, &body);
    }
    let tmp = dir.join(format!("{day}.md.tmp"));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, note_path(dir, day))?;
    Ok(Written::Logged)
}

/// Totals of a day's note; `None` when the note does not exist (a missing day is simply absent).
pub fn totals(dir: &Path, day: &str, low_kcal: u32) -> Option<Totals> {
    let text = read_note(dir, day).ok()??;
    Some(summarize(&text, low_kcal))
}

/// Where a photo lands, beside the day's note. The extension is the lower-cased original one.
pub fn photo_path(dir: &Path, day: &str, hhmm: &str, msg_id: u64, i: usize, name: &str) -> PathBuf {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "bin".to_string());
    dir.join(format!("{day}_{hhmm}_{msg_id}_{i}.{ext}"))
}

/// Writes a photo through a temp file. An existing non-empty file is left alone.
pub fn save_photo(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Each call gets its own temp name, so two handles of one photo never write the same file.
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let n = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.{n}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// `PIP_LOW_KCAL` if it parses as a whole number, else 1800 (a maintenance-oriented floor).
pub fn low_kcal_threshold() -> u32 {
    std::env::var("PIP_LOW_KCAL")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_LOW_KCAL)
}

/// The body of a note block: its heading, the optional one-line text, one line per photo.
fn note_block(n: &NoteMsg) -> String {
    let id = n.msg_id;
    let mut s = format!("<!-- pip:note {id} -->\n### {} · msg {id} · not a meal\n", one_line(&n.time));
    let text = one_line(&n.text);
    if !text.is_empty() {
        s.push_str(&format!("- note: {text}\n"));
    }
    for p in &n.photos {
        s.push_str(&format!("- photo: ![[{p}]]\n"));
    }
    s.push_str(&format!("<!-- /pip:note {id} -->\n"));
    s
}

/// Appends a non-meal message to the day's note. It never touches the totals. A message id already in
/// the note writes nothing.
pub fn write_note(dir: &Path, day: &str, n: &NoteMsg, _low_kcal: u32) -> io::Result<Written> {
    let _guard = dump::LOCK.lock().unwrap_or_else(|p| p.into_inner());
    fs::create_dir_all(dir)?;
    let existing = read_note(dir, day)?.unwrap_or_default();
    if existing.lines().any(|l| any_id(l) == Some(n.msg_id)) {
        return Ok(Written::Already);
    }
    let mut text = if existing.is_empty() {
        format!("# {day}\n\n{TOTALS_OPEN}\n{TOTALS_CLOSE}\n\n{MEALS_HEADING}\n\n")
    } else {
        existing
    };
    separate(&mut text);
    text.push_str(&note_block(n));
    let tmp = dir.join(format!("{day}.md.tmp"));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, note_path(dir, day))?;
    Ok(Written::Logged)
}

/// Each meal region as (id, its lines between the markers), in file order. A region ends at its own
/// close marker; a region left open by hand is dropped.
fn meal_regions(text: &str) -> Vec<(u64, Vec<&str>)> {
    let mut out = Vec::new();
    let mut open: Option<(u64, Vec<&str>)> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(id) = meal_open_id(t) {
            if open.is_none() {
                open = Some((id, Vec::new()));
            }
            continue;
        }
        let closes = matches!(&open, Some((id, _)) if meal_close_id(t) == Some(*id));
        if closes {
            if let Some(region) = open.take() {
                out.push(region);
            }
        } else if let Some((_, body)) = open.as_mut() {
            body.push(t);
        }
    }
    out
}

/// One meal row from a region's lines: the `### ` heading's time, the `- ` items before the macros line,
/// and the kcal of that macros line (0 when it is missing or unreadable).
fn meal_row(id: u64, body: &[&str]) -> MealRow {
    let time = body
        .iter()
        .find_map(|l| l.strip_prefix("### "))
        .map(|h| h.split(" · msg ").next().unwrap_or(h).to_string())
        .unwrap_or_default();
    let mut lines = Vec::new();
    let mut kcal = 0.0;
    for l in body {
        if let Some(rest) = l.strip_prefix("- macros: ") {
            kcal = parse_macros(rest).map(|(m, _)| m.kcal).unwrap_or(0.0);
            break;
        }
        if let Some(item) = l.strip_prefix("- ") {
            lines.push(item.to_string());
        }
    }
    MealRow { msg_id: id, time, kcal, lines }
}

/// The day's live meals in file order, first occurrence per id. Undone and note blocks are not listed.
pub fn meals(dir: &Path, day: &str) -> Vec<MealRow> {
    let Ok(Some(text)) = read_note(dir, day) else { return Vec::new() };
    let mut seen = HashSet::new();
    meal_regions(&text)
        .into_iter()
        .filter(|(id, _)| seen.insert(*id))
        .map(|(id, body)| meal_row(id, &body))
        .collect()
}

/// Marks the last meal that has a closed region as undone: its two marker lines become undone markers
/// and a `- undone:` line is inserted under the open one. No other byte changes except the totals
/// interior, which is re-rendered when it is Pip's own. `None` when the day has no meal.
pub fn undo_last(dir: &Path, day: &str, low_kcal: u32, by_msg: u64) -> io::Result<Option<MealRow>> {
    let _guard = dump::LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let Some(text) = read_note(dir, day)? else { return Ok(None) };
    let segs: Vec<&str> = text.split_inclusive('\n').collect();
    let mut target: Option<(usize, usize, u64)> = None;
    for (i, seg) in segs.iter().enumerate() {
        let Some(id) = meal_open_id(seg) else { continue };
        if let Some(j) = (i + 1..segs.len()).find(|&j| meal_close_id(segs[j]) == Some(id)) {
            target = Some((i, j, id));
        }
    }
    let Some((i, j, id)) = target else { return Ok(None) };
    let body: Vec<&str> = segs[i + 1..j].iter().map(|s| s.trim()).collect();
    let row = meal_row(id, &body);
    let mut out = String::with_capacity(text.len() + 64);
    for (k, seg) in segs.iter().enumerate() {
        if k == i {
            out.push_str(&format!("<!-- pip:undone {id} -->\n- undone: by msg {by_msg}\n"));
        } else if k == j {
            out.push_str(&format!("<!-- /pip:undone {id} -->\n"));
        } else {
            out.push_str(seg);
        }
    }
    if let Span::Ok(a, b) = totals_span(&out, low_kcal) {
        let body = totals_body(&summarize(&out, low_kcal), low_kcal);
        out.replace_range(a..b, &body);
    }
    let tmp = dir.join(format!("{day}.md.tmp"));
    fs::write(&tmp, out)?;
    fs::rename(&tmp, note_path(dir, day))?;
    Ok(Some(row))
}

/// Whether a command message was already acted on. The ledger is `.pip-commands` beside the notes.
pub fn command_seen(dir: &Path, msg_id: u64) -> bool {
    let want = msg_id.to_string();
    fs::read_to_string(dir.join(LEDGER)).map(|t| t.lines().any(|l| l.trim() == want)).unwrap_or(false)
}

/// Records a command message id before its action runs, appended under the same lock as the notes.
pub fn record_command(dir: &Path, msg_id: u64) -> io::Result<()> {
    let _guard = dump::LOCK.lock().unwrap_or_else(|p| p.into_inner());
    fs::create_dir_all(dir)?;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(dir.join(LEDGER))?;
    writeln!(f, "{msg_id}")
}
