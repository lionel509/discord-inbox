//! Pantry restock (#50): On Hand.md rows from a finished split note (his rows only) or a filed solo receipt. Writes only PANTRY.

use crate::dump;
use crate::vaults::{PANTRY, SPLITS};
use chrono::{Local, NaiveDate};
use std::fs;
use std::path::{Path, PathBuf};

/// Whole-word food names (lowercase word, canonical). This is both the normaliser and the food filter.
/// Nothing that is also a receipt word (total, tax, cash, member, price).
pub static FOODS: &[(&str, &str)] = &[
    ("cod", "cod"),
    ("salmon", "salmon"),
    ("tuna", "tuna"),
    ("shrimp", "shrimp"),
    ("tilapia", "tilapia"),
    ("chicken", "chicken"),
    ("beef", "beef"),
    ("pork", "pork"),
    ("eggs", "eggs"),
    ("egg", "eggs"),
    ("milk", "milk"),
    ("mlk", "milk"),
    ("butter", "butter"),
    ("cheese", "cheese"),
    ("yogurt", "yogurt"),
    ("tofu", "tofu"),
    ("kimchi", "kimchi"),
    ("rice", "rice"),
    ("noodles", "noodles"),
    ("noodle", "noodles"),
    ("ramen", "ramen"),
    ("bread", "bread"),
    ("onion", "onion"),
    ("garlic", "garlic"),
    ("ginger", "ginger"),
    ("scallion", "scallion"),
    ("potato", "potato"),
    ("carrot", "carrot"),
    ("broccoli", "broccoli"),
    ("spinach", "spinach"),
    ("mushroom", "mushroom"),
    ("tomato", "tomato"),
    ("apple", "apple"),
    ("banana", "banana"),
    ("avocado", "avocado"),
    ("lettuce", "lettuce"),
    ("cucumber", "cucumber"),
    ("cabbage", "cabbage"),
    ("orange", "orange"),
    ("grapes", "grapes"),
    ("grape", "grapes"),
    ("strawberries", "strawberries"),
    ("strawberry", "strawberries"),
    ("blueberries", "blueberries"),
    ("blueberry", "blueberries"),
];

/// A pantry command from a DM. Anything else goes to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Restock { date: Option<String>, all: bool },
    OutOf(String),
}

fn is_date(s: &str) -> bool {
    s.len() == 10 && s.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() })
}

/// `restock [all] [YYYY-MM-DD]` and `out of X`. A question ("out of milk?") or any other sentence is not a command.
pub fn command(content: &str) -> Option<Command> {
    let t = content.trim().to_lowercase();
    let restock_t = t.trim_end_matches('.').trim_end();
    if let Some(rest) = restock_t.strip_prefix("restock") {
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            return None;
        }
        let mut date = None;
        let mut all = false;
        for tok in rest.split_whitespace() {
            if tok == "all" {
                all = true;
            } else if is_date(tok) && date.is_none() {
                date = Some(tok.to_string());
            } else {
                return None;
            }
        }
        return Some(Command::Restock { date, all });
    }
    if let Some(rest) = t.strip_prefix("out of ") {
        // Any '?' in the trailing punctuation ("milk?", "milk?!") makes it a question.
        if t.trim_end().rsplit(|c: char| !c.is_ascii_punctuation()).next().is_some_and(|tail| tail.contains('?')) {
            return None;
        }
        let item = out_text(rest);
        if (1..=40).contains(&item.chars().count()) && out_parts(&item).is_some() {
            return Some(Command::OutOf(item));
        }
    }
    None
}

/// `out of` text without a leading article or trailing punctuation.
fn out_text(rest: &str) -> String {
    let mut s = rest.trim().trim_end_matches(|c: char| c.is_ascii_punctuation()).trim().to_lowercase();
    for art in ["the ", "my ", "some "] {
        if let Some(r) = s.strip_prefix(art) {
            s = r.to_string();
            break;
        }
    }
    s
}

/// The canonical food for one whole item name (`eggs`, `egg`, `tomatoes`), or None for anything longer.
fn whole_food(part: &str) -> Option<&'static str> {
    if part.is_empty() || !part.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let stem = part.strip_suffix('s');
    let stem_es = part.strip_suffix("es");
    FOODS.iter().find(|(word, _)| *word == part || Some(*word) == stem || Some(*word) == stem_es).map(|(_, canon)| *canon)
}

/// The canonical foods an `out of` text names: whole food items split on `,` and ` and `.
/// None when any piece is not a single food word, so "orange juice" never drops the orange row.
fn out_parts(item: &str) -> Option<Vec<&'static str>> {
    let mut out: Vec<&'static str> = Vec::new();
    for piece in item.split(',').flat_map(|p| p.split(" and ")) {
        let piece = piece.trim();
        if piece.is_empty() || piece == "and" {
            continue;
        }
        let canon = whole_food(piece)?;
        if !out.contains(&canon) {
            out.push(canon);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// The canonical food named in `text`: the first whole word that is a FOODS entry, or that entry plus `s` or `es`.
pub fn food(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    for w in lower.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()) {
        let stem = w.strip_suffix('s');
        let stem_es = w.strip_suffix("es");
        for (word, canon) in FOODS {
            if *word == w || Some(*word) == stem || Some(*word) == stem_es {
                return Some(*canon);
            }
        }
    }
    None
}

/// Receipt lines that name a payment, not a food: "APPLE PAY" must not read as apple.
fn is_tender_line(line: &str) -> bool {
    line.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphabetic())
        .any(|w| matches!(w, "pay" | "visa" | "mastercard" | "amex" | "debit" | "credit" | "tender" | "change"))
}

fn is_share(s: &str) -> bool {
    matches!(s, "½" | "⅓" | "⅔" | "¼" | "¾")
        || s.split_once('/').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()))
}

fn who_tokens(who: &str) -> Vec<&str> {
    who.split(|c: char| c.is_whitespace() || "·,+&".contains(c)).filter(|t| !t.is_empty()).collect()
}

/// What a split note says about him. `mine` is `(canonical, share)`, share "" for a whole item.
#[derive(Debug, Default)]
pub struct Split {
    pub mine: Vec<(String, String)>,
    pub others: usize,
    pub unassigned: Vec<String>,
}

/// Reads the first table with `Item` and `Who` columns. Only food rows count, and only rows whose Who names him.
pub fn parse_split(text: &str) -> Split {
    let mut out = Split::default();
    let mut cols: Option<(usize, usize)> = None;
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            if cols.is_some() && !t.is_empty() {
                break;
            }
            continue;
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
        let Some((ic, wc)) = cols else {
            if let (Some(i), Some(w)) = (cells.iter().position(|c| *c == "Item"), cells.iter().position(|c| *c == "Who")) {
                cols = Some((i, w));
            }
            continue;
        };
        if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
            continue;
        }
        if cells.len() <= ic.max(wc) {
            continue;
        }
        let Some(canon) = food(cells[ic]) else { continue };
        let toks = who_tokens(cells[wc]);
        if toks.is_empty() {
            out.unassigned.push(canon.to_string());
            continue;
        }
        match toks.iter().position(|t| t.to_ascii_lowercase().starts_with("lion")) {
            Some(i) => {
                let share = toks.get(i + 1).filter(|s| is_share(s)).map(|s| s.to_string()).unwrap_or_default();
                out.mine.push((canon.to_string(), share));
            }
            None => out.others += 1,
        }
    }
    out
}

/// The lines inside the fenced blocks that follow a `## OCR ` heading (dump::note_text shape).
fn ocr_lines(note: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut want = false;
    let mut fence: Option<String> = None;
    for line in note.lines() {
        if let Some(f) = fence.clone() {
            if line.trim() == f.as_str() {
                fence = None;
            } else {
                out.push(line);
            }
            continue;
        }
        if line.starts_with("## OCR ") {
            want = true;
            continue;
        }
        if want && !line.trim().is_empty() {
            want = false;
            let run: String = line.trim_start().chars().take_while(|c| *c == '~' || *c == '`').collect();
            if run.len() >= 3 {
                fence = Some(run);
            }
        }
    }
    out
}

/// Canonical food names on a receipt's OCR, deduped, first-seen order. Unmatched lines are skipped.
pub fn receipt_items(note: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in ocr_lines(note) {
        if is_tender_line(line) {
            continue;
        }
        if let Some(c) = food(line) {
            if !out.iter().any(|x| x.as_str() == c) {
                out.push(c.to_string());
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub item: String,
    pub qty: String,
    pub store: String,
    pub bought: String,
}

/// Rows after the `| Item |` header. Only the first four cells are kept; short rows are padded with empty cells.
pub fn parse_note(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
        if !in_table {
            in_table = cells.first() == Some(&"Item");
            continue;
        }
        if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
            continue;
        }
        if cells[0].is_empty() {
            continue;
        }
        let cell = |i: usize| cells.get(i).copied().unwrap_or("").to_string();
        rows.push(Row { item: cell(0), qty: cell(1), store: cell(2), bought: cell(3) });
    }
    rows
}

/// Same item (any case) replaces qty, store and bought in place, unless the new purchase is older; otherwise appends. Oldest first.
pub fn upsert(rows: &mut Vec<Row>, new: Vec<Row>) {
    for n in new {
        match rows.iter_mut().find(|r| r.item.eq_ignore_ascii_case(&n.item)) {
            Some(r) => {
                let older = matches!((NaiveDate::parse_from_str(&n.bought, "%Y-%m-%d"), NaiveDate::parse_from_str(&r.bought, "%Y-%m-%d")), (Ok(a), Ok(b)) if a < b);
                if !older {
                    r.qty = n.qty;
                    r.store = n.store;
                    r.bought = n.bought;
                }
            }
            None => rows.push(n),
        }
    }
    rows.sort_by(|a, b| (a.bought.as_str(), a.item.as_str()).cmp(&(b.bought.as_str(), b.item.as_str())));
}

/// Whole days from `bought` to `today`, or None when either date does not parse.
pub fn days(bought: &str, today: &str) -> Option<i64> {
    let b = NaiveDate::parse_from_str(bought, "%Y-%m-%d").ok()?;
    let t = NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    Some((t - b).num_days())
}

/// The whole note, rewritten by the bot. Prose outside the table is not kept.
pub fn render(rows: &[Row], today: &str) -> String {
    let mut s = format!(
        "# On Hand\n\n_Bought dates come from receipts; Days as of {today}. Say \"out of X\" to Stormy to drop a row._\n\n| Item | Qty | Store | Bought | Days |\n|---|---|---|---|---|\n"
    );
    for r in rows {
        let d = days(&r.bought, today).map(|d| d.to_string()).unwrap_or_else(|| "?".into());
        s.push_str(&format!("| {} | {} | {} | {} | {} |\n", r.item, r.qty, r.store, r.bought, d));
    }
    s.push_str("\n**Hub:** [[Pantry]]\n");
    s
}

/// What a restock did, for the DM reply.
#[derive(Debug)]
pub struct Report {
    pub source: String,
    pub added: Vec<String>,
    pub others: usize,
    pub unassigned: Vec<String>,
    pub oldest: Vec<(String, i64)>,
}

impl Report {
    pub fn text(&self) -> String {
        let mut s = if self.added.is_empty() {
            format!("Nothing food-shaped to add from {}.", self.source)
        } else {
            let parts: Vec<String> = self.added.iter().map(|a| format!("+{a}")).collect();
            format!("Pantry restocked from {}: {} 🐾", self.source, parts.join(", "))
        };
        if self.others > 0 {
            s.push_str(&format!("\nSkipped {} rows for other people.", self.others));
        }
        if !self.unassigned.is_empty() {
            s.push_str(&format!("\nNo initials on: {}. Fix the split note and say restock again.", self.unassigned.join(", ")));
        }
        if !self.oldest.is_empty() {
            let v: Vec<String> = self.oldest.iter().take(3).map(|(i, d)| format!("{i} {d}d")).collect();
            s.push_str(&format!("\nOldest on hand: {}.", v.join(", ")));
        }
        s
    }
}

fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn is_regular(p: &Path) -> bool {
    fs::symlink_metadata(p).map(|m| m.is_file()).unwrap_or(false)
}

/// A pantry note that exists must be a regular file, never a link.
fn refuse_link(p: &Path) -> Result<(), String> {
    match fs::symlink_metadata(p) {
        Ok(m) if !m.is_file() => Err("refusing to write: the pantry note is not a regular file".into()),
        _ => Ok(()),
    }
}

fn oldest(rows: &[Row], today: &str) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = rows.iter().filter_map(|r| days(&r.bought, today).map(|d| (r.item.clone(), d))).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

fn write_note(root: &Path, rows: &[Row], today: &str) -> Result<(), String> {
    let dir: PathBuf = root.join("Fidelity/Pantry");
    let tmp = dir.join(".On Hand.md.tmp");
    fs::write(&tmp, render(rows, today)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, root.join(PANTRY)).map_err(|e| e.to_string())
}

/// `restock` for `date`, as of today's local date.
pub fn restock(root: &Path, date: &str, all: bool) -> Result<Report, String> {
    restock_at(root, date, all, &today())
}

fn restock_at(root: &Path, date: &str, all: bool, today: &str) -> Result<Report, String> {
    let _guard = dump::LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if !root.join("Fidelity/Pantry").is_dir() {
        return Err("there is no Fidelity/Pantry folder in the vault".into());
    }
    let note_path = root.join(PANTRY);
    refuse_link(&note_path)?;

    let split_path = root.join(SPLITS).join(format!("Costco Split — {date}.md"));
    let (new_rows, source, others, unassigned) = if is_regular(&split_path) {
        let text = fs::read_to_string(&split_path).map_err(|e| e.to_string())?;
        let s = parse_split(&text);
        let source = split_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let rows: Vec<Row> = s
            .mine
            .iter()
            .map(|(c, share)| Row { item: c.clone(), qty: share.clone(), store: "Costco".into(), bought: date.into() })
            .collect();
        (rows, source, s.others, s.unassigned)
    } else {
        let dir = root.join("State Street");
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .collect();
        paths.sort();
        let prefix = format!("{date} ");
        let mut filed = 0usize;
        // (canonical food, title of the note it came from). First note to name a food wins.
        let mut found: Vec<(String, String)> = Vec::new();
        let mut costco = false;
        for p in paths {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Some(stem) = name.strip_suffix(".md") else { continue };
            let Some(title) = stem.strip_prefix(&prefix) else { continue };
            if !is_regular(&p) {
                continue;
            }
            let text = fs::read_to_string(&p).unwrap_or_default();
            let ocr = ocr_lines(&text);
            costco |= ocr.iter().any(|l| { let m = l.to_lowercase().replace('0', "o"); m.contains("costco") || m.contains("kirkland") });
            filed += 1;
            for c in receipt_items(&text) {
                if !found.iter().any(|(f, _)| *f == c) {
                    found.push((c, title.to_string()));
                }
            }
        }
        if filed == 0 {
            return Err(format!("no split note or receipt filed for {date}. Say `restock YYYY-MM-DD` for another day."));
        }
        if costco && !all {
            return Err(format!(
                "that looks like a Costco receipt and there is no split note for {date} yet. Write the split first, then say restock (or `restock all` if it was a solo trip)."
            ));
        }
        let mut sources: Vec<String> = Vec::new();
        let mut rows: Vec<Row> = Vec::new();
        for (c, title) in found {
            if !sources.contains(&title) {
                sources.push(title.clone());
            }
            rows.push(Row { item: c, qty: String::new(), store: title, bought: date.into() });
        }
        let source = if sources.is_empty() { format!("the receipts filed for {date}") } else { sources.join(", ") };
        (rows, source, 0, Vec::new())
    };

    let mut rows = parse_note(&fs::read_to_string(&note_path).unwrap_or_default());
    let mut added: Vec<String> = Vec::new();
    for r in &new_rows {
        if !added.contains(&r.item) {
            added.push(r.item.clone());
        }
    }
    if !new_rows.is_empty() {
        upsert(&mut rows, new_rows);
        write_note(root, &rows, today)?;
    } else {
        added.clear();
    }
    Ok(Report { source, added, others, unassigned, oldest: oldest(&rows, today) })
}

/// Drops the row for each food the item names ("eggs and milk"). Anything that is not whole food names is refused.
pub fn out_of(root: &Path, item: &str, today: &str) -> Result<String, String> {
    let Some(keys) = out_parts(&out_text(item)) else {
        return Err("I can only drop whole food items, like `out of eggs and milk`.".into());
    };
    let _guard = dump::LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let note_path = root.join(PANTRY);
    if fs::symlink_metadata(&note_path).is_err() {
        return Ok("No pantry note yet.".into());
    }
    refuse_link(&note_path)?;
    let mut rows = parse_note(&fs::read_to_string(&note_path).map_err(|e| e.to_string())?);
    let mut dropped: Vec<&str> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    for k in keys {
        let before = rows.len();
        rows.retain(|r| !r.item.eq_ignore_ascii_case(k));
        if rows.len() < before {
            dropped.push(k);
        } else {
            missing.push(k);
        }
    }
    if !dropped.is_empty() {
        write_note(root, &rows, today)?;
    }
    let mut reply = if dropped.is_empty() { String::new() } else { format!("Dropped {} from the pantry 🐾", dropped.join(", ")) };
    if !missing.is_empty() {
        let m = format!("No {} in the pantry right now.", missing.join(", "));
        if reply.is_empty() {
            reply = m;
        } else {
            reply.push('\n');
            reply.push_str(&m);
        }
    }
    Ok(reply)
}

/// Runs a command against the vault at `root`. `today` is the sender's local date.
pub fn run(root: &Path, cmd: Command, today: &str) -> Result<String, String> {
    match cmd {
        Command::Restock { date, all } => {
            let d = date.unwrap_or_else(|| today.to_string());
            restock_at(root, &d, all, today).map(|r| r.text())
        }
        Command::OutOf(item) => out_of(root, &item, today),
    }
}
