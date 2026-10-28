//! Todo files for the daily brief (#135): dated day files under `! Todo/`, open notes, strikes, carry-over and the check-in log. Stdlib plus chrono and `redact`; no file outside `dir` is touched.
use chrono::NaiveDate;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

pub const DIR: &str = "! Todo";
pub const CHECKIN_LOG: &str = "Stormy Check-ins.md";
const HEADER: &str = "---\ntags:\n  - todo\n---\n";
// Serialises the read-modify-write of day files and the check-in log within this process: two replies at once must not lose an entry or race on one file.
static WRITE: Mutex<()> = Mutex::new(());
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn write_lock() -> MutexGuard<'static, ()> {
    WRITE.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Line { file: PathBuf, idx: usize },
    Note { file: PathBuf },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub text: String,
    pub place: Place,
}

pub fn day_file(dir: &Path, day: NaiveDate) -> PathBuf {
    dir.join(format!("{}.md", day.format("%Y-%m-%d")))
}

/// Index of the first body line: 0 without front matter, else the line after the closing `---`.
fn body_start(lines: &[&str]) -> (usize, Option<usize>) {
    if lines.first().map(|l| l.trim_start_matches('\u{feff}').trim()) != Some("---") {
        return (0, None);
    }
    match lines.iter().skip(1).position(|l| l.trim() == "---") {
        Some(p) => (p + 2, Some(p + 1)),
        None => (0, None),
    }
}

/// Strip a bullet marker from an open line.
fn bullet_text(trimmed: &str) -> &str {
    let trimmed = trimmed.trim_start_matches('\u{feff}');
    let t = trimmed
        .strip_prefix("- [ ] ")
        .or_else(|| trimmed.strip_prefix("* [ ] "))
        .or_else(|| trimmed.strip_prefix("- "))
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed);
    t.trim()
}

fn is_open(trimmed: &str) -> bool {
    let trimmed = trimmed.trim_start_matches('\u{feff}').trim();
    let t = bullet_text(trimmed);
    // An item needs some text: a bare box, a rule line (`---`, `***`) or an empty bullet is not one. A ticked box or a struck item is done.
    let has_text = t.chars().any(|c| c.is_alphanumeric());
    let done = t.starts_with("[x]") || t.starts_with("[X]") || t.starts_with("~~");
    !trimmed.is_empty() && has_text && !done && !trimmed.starts_with('#') && !trimmed.starts_with("Hub:") && !trimmed.starts_with('>')
}

/// Normalised form used for duplicate checks: no bullet, no strike marks, trimmed.
fn norm(line: &str) -> String {
    let t = bullet_text(line.trim());
    t.trim_matches('~').trim().to_lowercase()
}

pub fn day_items(dir: &Path, day: NaiveDate) -> Vec<Item> {
    let file = day_file(dir, day);
    let Ok(body) = fs::read_to_string(&file) else {
        return Vec::new();
    };
    let lines: Vec<&str> = body.lines().collect();
    let (start, _) = body_start(&lines);
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate().skip(start) {
        let t = line.trim();
        if !is_open(t) {
            continue;
        }
        let text = bullet_text(t);
        if text.is_empty() {
            continue;
        }
        out.push(Item { text: text.to_string(), place: Place::Line { file: file.clone(), idx } });
    }
    out
}

/// True when the front matter (if any) carries a `done:` key.
fn has_done(lines: &[&str]) -> bool {
    let (start, close) = body_start(lines);
    match close {
        Some(c) => lines[1..c].iter().any(|l| l.starts_with("done:")) && start > 0,
        None => false,
    }
}

pub fn note_items(dir: &Path) -> Vec<Item> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        // A symlink is not a note: its target is not ours to read or rewrite.
        .filter(|p| fs::symlink_metadata(p).map(|m| m.file_type().is_file()).unwrap_or(false) && p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for p in files {
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if p.file_name().and_then(|s| s.to_str()) == Some(CHECKIN_LOG) {
            continue;
        }
        if NaiveDate::parse_from_str(stem, "%Y-%m-%d").is_ok() {
            continue;
        }
        let Ok(body) = fs::read_to_string(&p) else {
            continue;
        };
        let lines: Vec<&str> = body.lines().collect();
        if has_done(&lines) {
            continue;
        }
        out.push(Item { text: stem.to_string(), place: Place::Note { file: p.clone() } });
    }
    out
}

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent dir")?;
    let name = path.file_name().and_then(|s| s.to_str()).ok_or("bad file name")?;
    let n = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.tmp-{}-{n}", std::process::id()));
    fs::write(&tmp, content).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
}

pub fn mark_done(item: &Item, day: NaiveDate) -> Result<(), String> {
    let _write = write_lock();
    match &item.place {
        Place::Line { file, idx } => {
            let body = fs::read_to_string(file).map_err(|e| format!("read {}: {e}", file.display()))?;
            let mut lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
            let line = lines.get(*idx).ok_or("line index out of range")?.clone();
            // Already struck (a repeat reply, or two at once): leave it as it is.
            if bullet_text(line.trim()).starts_with("~~") {
                return Ok(());
            }
            let text = bullet_text(line.trim());
            lines[*idx] = format!("~~{text}~~");
            let mut out = lines.join("\n");
            out.push('\n');
            write_atomic(file, &out)
        }
        Place::Note { file } => {
            let body = fs::read_to_string(file).map_err(|e| format!("read {}: {e}", file.display()))?;
            let stamp = format!("done: {}", day.format("%Y-%m-%d"));
            let lines: Vec<&str> = body.lines().collect();
            let (_, close) = body_start(&lines);
            let out = match close {
                Some(c) => {
                    let mut v: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
                    v.insert(c, stamp);
                    let mut s = v.join("\n");
                    s.push('\n');
                    s
                }
                None => format!("---\n{stamp}\n---\n{body}"),
            };
            write_atomic(file, &out)
        }
    }
}

pub fn carry(dir: &Path, day: NaiveDate, texts: &[String]) -> Result<usize, String> {
    let _write = write_lock();
    let file = day_file(dir, day);
    let existing = fs::read_to_string(&file).unwrap_or_default();
    let mut seen: Vec<String> = existing.lines().map(norm).filter(|s| !s.is_empty()).collect();
    let mut body = if existing.is_empty() { HEADER.to_string() } else { existing.clone() };
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    let mut added = 0;
    for t in texts {
        let clean = t.replace(['\n', '\r'], " ").trim().to_string();
        if clean.is_empty() || seen.iter().any(|s| *s == norm(&clean)) {
            continue;
        }
        body.push_str(&format!("- {clean}\n"));
        seen.push(norm(&clean));
        added += 1;
    }
    if added > 0 {
        fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        write_atomic(&file, &body)?;
    }
    Ok(added)
}

pub fn log_checkin(dir: &Path, day: NaiveDate, reply: &str, done: &[String], carried: &[String]) -> Result<(), String> {
    let _write = write_lock();
    let file = dir.join(CHECKIN_LOG);
    let mut body = fs::read_to_string(&file).unwrap_or_else(|_| HEADER.to_string());
    if !body.ends_with('\n') {
        body.push('\n');
    }
    let entry = format!(
        "## {}\n{}\nDone: {}\nCarried: {}\n\n",
        day.format("%Y-%m-%d"),
        crate::redact::sensitive(reply),
        done.join(", "),
        carried.join(", ")
    );
    body.push_str(&entry);
    fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    write_atomic(&file, &body)
}
