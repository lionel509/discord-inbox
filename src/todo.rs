//! Todo files for the daily brief (#135): dated day files, note files, strikes, carry-over and the check-in log. Only appends or strikes; never deletes a line.
use chrono::NaiveDate;
use std::path::{Path, PathBuf};

pub const DIR: &str = "! Todo";
pub const CHECKIN_LOG: &str = "Stormy Check-ins.md";
const HEADER: &str = "---\ntags:\n  - todo\n---\n";

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

/// Number of leading lines taken by a `---` front-matter block (0 when there is none).
fn front_len(lines: &[&str]) -> usize {
    if lines.first().map(|l| l.trim()) != Some("---") {
        return 0;
    }
    match lines.iter().skip(1).position(|l| l.trim() == "---") {
        Some(p) => p + 2,
        None => 0,
    }
}

/// The open item text of a day-file line, or `None` for blanks, headings, hub lines and struck lines.
fn item_text(line: &str) -> Option<String> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with("Hub:") || t.starts_with("~~") {
        return None;
    }
    let t = t.strip_prefix("- ").unwrap_or(t).trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub fn day_items(dir: &Path, day: NaiveDate) -> Vec<Item> {
    let file = day_file(dir, day);
    let Ok(body) = std::fs::read_to_string(&file) else { return Vec::new() };
    let lines: Vec<&str> = body.lines().collect();
    let start = front_len(&lines);
    lines
        .iter()
        .enumerate()
        .skip(start)
        .filter_map(|(idx, l)| item_text(l).map(|text| Item { text, place: Place::Line { file: file.clone(), idx } }))
        .collect()
}

fn is_day_name(stem: &str) -> bool {
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").is_ok()
}

/// Notes in the folder whose front matter has no `done:` key. Day files and the check-in log are not notes.
pub fn note_items(dir: &Path) -> Vec<Item> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Item> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.to_string();
            let stem = name.strip_suffix(".md")?.to_string();
            if name == CHECKIN_LOG || is_day_name(&stem) {
                return None;
            }
            let body = std::fs::read_to_string(&path).ok()?;
            let lines: Vec<&str> = body.lines().collect();
            let fl = front_len(&lines);
            if lines.iter().take(fl).any(|l| l.trim_start().starts_with("done:")) {
                return None;
            }
            Some(Item { text: stem, place: Place::Note { file: path } })
        })
        .collect();
    out.sort_by(|a, b| a.text.cmp(&b.text));
    out
}

/// Strikes the item's line (`~~text~~`) or stamps `done: DATE` into the note's front matter.
pub fn mark_done(item: &Item, day: NaiveDate) -> Result<(), String> {
    match &item.place {
        Place::Line { file, idx } => {
            let body = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            let mut lines: Vec<String> = body.lines().map(String::from).collect();
            let cur = lines.get(*idx).ok_or("line moved")?;
            if item_text(cur).as_deref() != Some(item.text.as_str()) {
                return Err("line changed since it was listed".to_string());
            }
            lines[*idx] = format!("~~{}~~", item.text);
            let mut out = lines.join("\n");
            if body.ends_with('\n') {
                out.push('\n');
            }
            std::fs::write(file, out).map_err(|e| e.to_string())
        }
        Place::Note { file } => {
            let body = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            let stamp = format!("done: {}", day.format("%Y-%m-%d"));
            let lines: Vec<&str> = body.lines().collect();
            let fl = front_len(&lines);
            let out = if fl == 0 {
                format!("---\n{stamp}\n---\n{body}")
            } else {
                let mut v: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
                v.insert(fl - 1, stamp);
                let mut s = v.join("\n");
                if body.ends_with('\n') {
                    s.push('\n');
                }
                s
            };
            std::fs::write(file, out).map_err(|e| e.to_string())
        }
    }
}

/// Appends each text not already on `day`'s list as `- text`. Creates the file with todo front matter. Returns how many were added.
pub fn carry(dir: &Path, day: NaiveDate, texts: &[String]) -> Result<usize, String> {
    let file = day_file(dir, day);
    let mut have: Vec<String> = day_items(dir, day).into_iter().map(|i| i.text).collect();
    let mut body = std::fs::read_to_string(&file).unwrap_or_else(|_| HEADER.to_string());
    let mut added = 0;
    for t in texts {
        let t = t.trim();
        if t.is_empty() || have.iter().any(|h| h == t) {
            continue;
        }
        if !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&format!("- {t}\n"));
        have.push(t.to_string());
        added += 1;
    }
    if added > 0 || !file.exists() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(&file, body).map_err(|e| e.to_string())?;
    }
    Ok(added)
}

/// Appends one `## DATE` entry to the check-in log, every night.
pub fn log_checkin(dir: &Path, day: NaiveDate, reply: &str, done: &[String], carried: &[String]) -> Result<(), String> {
    let file = dir.join(CHECKIN_LOG);
    let mut body = std::fs::read_to_string(&file).unwrap_or_else(|_| HEADER.to_string());
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&format!(
        "\n## {}\nReply: {}\nDone: {}\nCarried: {}\n",
        day.format("%Y-%m-%d"),
        reply.trim().replace('\n', " "),
        done.join(", "),
        carried.join(", ")
    ));
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(&file, body).map_err(|e| e.to_string())
}
