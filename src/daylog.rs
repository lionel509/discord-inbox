//! Day notes (U3): one markdown note per local day, one `### ` block per meal, and a header whose
//! totals are recomputed from the `- macros:` lines. Writes run under `dump::LOCK`; reads create nothing.
use crate::dump;
use crate::nutrition::Macros;
use std::fs;
use std::io::{self, ErrorKind};
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

const MEALS_HEADING: &str = "## Meals";
const DEFAULT_LOW_KCAL: u32 = 1800;

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

/// True when a meal heading in `text` carries this message id.
fn has_msg(text: &str, msg_id: u64) -> bool {
    let tail = format!("· msg {msg_id}");
    text.lines().any(|l| l.starts_with("### ") && l.trim_end().ends_with(&tail))
}

pub fn logged(dir: &Path, day: &str, msg_id: u64) -> bool {
    matches!(read_note(dir, day), Ok(Some(t)) if has_msg(&t, msg_id))
}

pub fn has_meal(dir: &Path, day: &str) -> bool {
    matches!(read_note(dir, day), Ok(Some(t)) if t.lines().any(|l| l.starts_with("### ")))
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

fn meal_block(meal: &Meal) -> String {
    let mut s = format!("### {} · msg {}\n", one_line(&meal.time), meal.msg_id);
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
    s.push('\n');
    s
}

/// An existing note without its old header: the front matter, the `# ` title (its first line after the
/// front matter), and the `**Total:**` / `**Flags:**` lines before the first section. Everything else is
/// kept, including text the user typed above `## Meals`, and leading blank lines are dropped.
fn note_rest(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut start = 0;
    if lines.first() == Some(&"---") {
        if let Some(end) = lines[1..].iter().position(|l| *l == "---") {
            start = end + 2;
        }
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut in_header = true;
    for (i, line) in lines.iter().enumerate().skip(start) {
        if in_header {
            if i == start && line.starts_with("# ") {
                continue;
            }
            if line.starts_with("**Total:**") || line.starts_with("**Flags:**") {
                continue;
            }
            if line.starts_with("## ") || line.starts_with("### ") {
                in_header = false;
            }
        }
        kept.push(line);
    }
    let mut out = String::new();
    for line in kept.iter().skip_while(|l| l.trim().is_empty()) {
        out.push_str(line);
        out.push('\n');
    }
    out
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

/// Sums every `- macros:` line of a note (whole numbers) and derives the flags.
fn summarize(text: &str, low_kcal: u32) -> Totals {
    let mut sum = Macros::default();
    let (mut meals, mut veg) = (0usize, false);
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("- macros: ") else { continue };
        let Some((m, v)) = parse_macros(rest) else { continue };
        sum.kcal += m.kcal;
        sum.protein += m.protein;
        sum.carbs += m.carbs;
        sum.fat += m.fat;
        meals += 1;
        veg |= v;
    }
    let mut flags = Vec::new();
    if meals >= 1 && !veg {
        flags.push("no-veg");
    }
    if sum.kcal < low_kcal as f64 {
        flags.push("low-kcal");
    }
    Totals { macros: sum, meals, veg, flags }
}

fn header(day: &str, t: &Totals, low_kcal: u32) -> String {
    let k = t.macros.kcal as i64;
    let p = t.macros.protein as i64;
    let c = t.macros.carbs as i64;
    let f = t.macros.fat as i64;
    let n = t.meals;
    let s = if n == 1 { "" } else { "s" };
    let v = yes_no(t.veg);
    let flags_yaml = if t.flags.is_empty() { "none".to_string() } else { t.flags.join(", ") };
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
    format!(
        "---\ndate: {day}\nkcal: {k}\nprotein_g: {p}\ncarbs_g: {c}\nfat_g: {f}\nmeals: {n}\nveg: {v}\nflags: {flags_yaml}\n---\n# {day}\n**Total:** {k} kcal · P {p} g · C {c} g · F {f} g · {n} meal{s} · veg {v}\n**Flags:** {flags_md}\n\n"
    )
}

/// Appends one meal to the day's note. A message id already in the note writes nothing.
pub fn write_meal(dir: &Path, day: &str, meal: &Meal, low_kcal: u32) -> io::Result<Written> {
    let _guard = dump::LOCK.lock().unwrap_or_else(|p| p.into_inner());
    fs::create_dir_all(dir)?;
    let existing = read_note(dir, day)?.unwrap_or_default();
    if has_msg(&existing, meal.msg_id) {
        return Ok(Written::Already);
    }
    let mut rest = note_rest(&existing);
    if !rest.lines().any(|l| l.trim_end() == MEALS_HEADING) {
        rest.push_str(MEALS_HEADING);
        rest.push_str("\n\n");
    }
    rest.push_str(&meal_block(meal));
    let text = header(day, &summarize(&rest, low_kcal), low_kcal) + &rest;
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
