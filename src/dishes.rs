//! #159: recipe notes with a kcal quoted in them, read by Pip as dish candidates.
use std::fs;
use std::path::{Path, PathBuf};

/// One recipe note with the lines that quote kcal.
#[derive(Debug, Clone, PartialEq)]
pub struct Dish {
    pub note: String,
    pub lines: Vec<String>,
}

const MAX_NOTES: usize = 40;
const MAX_LINES: usize = 8;
const MAX_CHARS: usize = 160;

/// Dishes from `PIP_DISH_NOTES` paths: a folder gives its own `*.md` files, a file is one note.
pub fn load(paths: &[PathBuf]) -> Vec<Dish> {
    let mut out: Vec<Dish> = Vec::new();
    for p in paths {
        let Ok(meta) = fs::symlink_metadata(p) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() {
            if let Some(d) = read_note(p) {
                out.push(d);
            }
        } else if meta.is_dir() {
            let Ok(rd) = fs::read_dir(p) else { continue };
            for e in rd.filter_map(|e| e.ok()) {
                let path = e.path();
                let Ok(m) = fs::symlink_metadata(&path) else { continue };
                if m.file_type().is_symlink() || !m.is_file() {
                    continue;
                }
                if path.extension().and_then(|x| x.to_str()) != Some("md") {
                    continue;
                }
                if let Some(d) = read_note(&path) {
                    out.push(d);
                }
            }
        }
    }
    out.sort_by(|a, b| a.note.cmp(&b.note));
    out.truncate(MAX_NOTES);
    out
}

fn read_note(path: &Path) -> Option<Dish> {
    let text = fs::read_to_string(path).ok()?;
    let lines = lines_of(&text);
    if lines.is_empty() {
        return None;
    }
    let note = path.file_stem()?.to_str()?.to_string();
    Some(Dish { note, lines })
}

/// The kcal lines of a note body (front matter skipped), each cleaned and capped.
pub fn lines_of(text: &str) -> Vec<String> {
    let (_, body) = crate::recipes::split_frontmatter(text);
    let mut out: Vec<String> = Vec::new();
    let mut kcal_col: Option<usize> = None;
    for raw in body.lines() {
        let t = raw.trim();
        if t.starts_with('|') {
            let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
            let separator = cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'));
            if separator {
                continue;
            }
            if let Some(col) = cells.iter().position(|c| c.to_lowercase().contains("kcal")) {
                kcal_col = Some(col);
                continue;
            }
            if let Some(col) = kcal_col {
                if let (Some(first), Some(val)) = (cells.first(), cells.get(col)) {
                    if !first.is_empty() && !val.is_empty() {
                        let v = if val.to_lowercase().contains("kcal") { val.to_string() } else { format!("{val} kcal") };
                        push_clean(&mut out, &format!("{first}: {v}"));
                    }
                }
            }
            continue;
        }
        kcal_col = None;
        if raw.to_lowercase().contains("kcal") {
            push_clean(&mut out, raw);
        }
    }
    out
}

fn push_clean(out: &mut Vec<String>, raw: &str) {
    if out.len() >= MAX_LINES {
        return;
    }
    let s = clean(raw);
    if s.is_empty() || !s.to_lowercase().contains("kcal") {
        return;
    }
    let s: String = crate::redact::sensitive(&s).chars().take(MAX_CHARS).collect();
    let s = s.trim().to_string();
    if !s.is_empty() {
        out.push(s);
    }
}

/// Strips heading, quote and list markers, emphasis, and wiki-link brackets (`[[a|b]]` -> `b`, `[[a]]` -> `a`).
fn clean(raw: &str) -> String {
    let mut s = raw.trim().trim_start_matches(|c: char| c == '#' || c == '-' || c == '>' || c == ' ');
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &s[digits..];
        if rest.starts_with(". ") || rest.starts_with(") ") {
            s = rest[2..].trim_start();
        }
    }
    let s = s.replace('*', "");
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find("[[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("]]") {
            Some(j) => {
                let inner = &after[..j];
                out.push_str(inner.rsplit('|').next().unwrap_or(inner));
                rest = &after[j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Number tokens in `line` as (value, byte start, byte end). Dates like `2026-11-04` are not numbers.
fn number_tokens(line: &str) -> Vec<(f64, usize, usize)> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let b: Vec<char> = chars.iter().map(|&(_, c)| c).collect();
    let byte_at = |i: usize| if i < chars.len() { chars[i].0 } else { line.len() };
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && b[i - 1].is_ascii_alphanumeric()) {
            i += 1;
            continue;
        }
        let start = i;
        let mut num = String::new();
        let mut j = i;
        while j < b.len() {
            if b[j].is_ascii_digit() {
                num.push(b[j]);
                j += 1;
            } else if b[j] == ',' && j + 1 < b.len() && b[j + 1].is_ascii_digit() {
                j += 1;
            } else {
                break;
            }
        }
        if j + 1 < b.len() && b[j] == '.' && b[j + 1].is_ascii_digit() {
            num.push('.');
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                num.push(b[j]);
                j += 1;
            }
        }
        let date_before = start >= 2 && b[start - 1] == '-' && b[start - 2].is_ascii_digit();
        let date_after = j + 1 < b.len() && b[j] == '-' && b[j + 1].is_ascii_digit();
        if !date_before && !date_after {
            if let Ok(v) = num.parse::<f64>() {
                out.push((v, byte_at(start), byte_at(j)));
            }
        }
        i = j.max(i + 1);
    }
    out
}

/// True when `kcal` is a number quoted on one of the dish's lines (a single number, or a range that holds it).
pub fn quoted(dish: &Dish, kcal: f64) -> bool {
    if !(1.0..=5000.0).contains(&kcal) {
        return false;
    }
    dish.lines.iter().any(|line| {
        let toks = number_tokens(line);
        if toks.iter().any(|(v, _, _)| (v - kcal).abs() <= 0.5) {
            return true;
        }
        toks.windows(2).any(|w| {
            let (a, _, a_end) = w[0];
            let (b, b_start, _) = w[1];
            let gap = line[a_end..b_start].trim();
            matches!(gap, "-" | "–" | "—" | "to") && a <= kcal && kcal <= b
        })
    })
}

/// The one dish whose note name matches `mark`; `None` for no match or several.
pub fn marked<'a>(mark: &str, dishes: &'a [Dish]) -> Option<&'a Dish> {
    let want = crate::foods::norm(mark);
    if want.is_empty() {
        return None;
    }
    let mut hits = dishes.iter().filter(|d| crate::foods::norm(&d.note) == want);
    let first = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    Some(first)
}
