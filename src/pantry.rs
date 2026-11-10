//! Pantry restock (#50): On Hand.md rows from a finished split note (his rows only) or a filed solo receipt. Writes only PANTRY.

use crate::dump;
use crate::vaults::{PANTRY, SPLITS};
use crate::{foods, ocr, redact};
use chrono::{Local, NaiveDate};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    render_with(rows, &[], today)
}

/// `render` plus the products block (#159). With no products the output is `render`'s, byte for byte.
pub fn render_with(rows: &[Row], products: &[Product], today: &str) -> String {
    let mut s = format!(
        "# On Hand\n\n_Bought dates come from receipts; Days as of {today}. Say \"out of X\" to Stormy to drop a row._\n\n| Item | Qty | Store | Bought | Days |\n|---|---|---|---|---|\n"
    );
    for r in rows {
        let d = days(&r.bought, today).map(|d| d.to_string()).unwrap_or_else(|| "?".into());
        s.push_str(&format!("| {} | {} | {} | {} | {} |\n", r.item, r.qty, r.store, r.bought, d));
    }
    let lines: Vec<String> = products.iter().filter_map(product_line).collect();
    if !lines.is_empty() {
        s.push_str(&format!("\n## Products\n\n{PRODUCTS_OPEN}\n"));
        for l in &lines {
            s.push_str(l);
            s.push('\n');
        }
        s.push_str(&format!("{PRODUCTS_CLOSE}\n"));
    }
    s.push_str("\nHub: [[Pantry]]\n");
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
            format!("Pantry restocked from {}: {}", self.source, parts.join(", "))
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

/// The restock reply: `Report::text`, plus the products added (#159).
fn reply(r: &Report, products: &[String]) -> String {
    if products.is_empty() {
        return r.text();
    }
    let mut s = r.text();
    if r.added.is_empty() {
        s = s.replacen(&format!("Nothing food-shaped to add from {}.", r.source), &format!("Pantry restocked from {}.", r.source), 1);
    }
    s.push_str(&format!("\nProducts: {}.", products.join("; ")));
    s
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

fn write_note(root: &Path, rows: &[Row], products: &[Product], today: &str) -> Result<(), String> {
    let dir: PathBuf = root.join("Fidelity/Pantry");
    let tmp = dir.join(".On Hand.md.tmp");
    fs::write(&tmp, render_with(rows, products, today)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, root.join(PANTRY)).map_err(|e| e.to_string())
}

/// `restock` for `date`, as of today's local date.
pub fn restock(root: &Path, date: &str, all: bool) -> Result<Report, String> {
    with_helper(|ex| restock_with(root, date, all, &today(), ex)).map(|(r, _)| r)
}

/// The extractor (a slow model call) runs before the lock, which is taken only for the note write.
/// Returns the report and the product names newly added (#159).
fn restock_with(root: &Path, date: &str, all: bool, today: &str, extract: Option<&dyn Fn(&str) -> Option<String>>) -> Result<(Report, Vec<String>), String> {
    let split_path = root.join(SPLITS).join(format!("Costco Split — {date}.md"));
    let split = is_regular(&split_path);
    let mut new_products: Vec<Product> = Vec::new();
    if let (Some(ex), false) = (extract, split) {
        let receipts = filed_receipts(root, date);
        let costco = receipts.iter().any(|(_, t)| costco_text(t));
        if !receipts.is_empty() && (all || !costco) {
            for (_, text) in &receipts {
                if let Some(reply) = ex(&extract_prompt(text)) {
                    upsert_products(&mut new_products, parse_extract(&reply, date));
                }
            }
        }
    }
    let _guard = dump::LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if !root.join("Fidelity/Pantry").is_dir() {
        return Err("there is no Fidelity/Pantry folder in the vault".into());
    }
    let note_path = root.join(PANTRY);
    refuse_link(&note_path)?;

    let (new_rows, source, others, unassigned) = if split {
        let text = fs::read_to_string(&split_path).map_err(|e| e.to_string())?;
        let s = parse_split(&text);
        if extract.is_some() {
            for (c, _) in &s.mine {
                upsert_products(&mut new_products, vec![Product { name: c.clone(), brand: "Costco".into(), size: String::new(), bought: date.into() }]);
            }
        }
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
            costco |= costco_text(&text);
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

    let old = fs::read_to_string(&note_path).unwrap_or_default();
    let mut rows = parse_note(&old);
    let mut have = parse_products(&old);
    let mut product_names: Vec<String> = Vec::new();
    for p in &new_products {
        let key = foods::norm(&p.name);
        if !have.iter().any(|h| foods::norm(&h.name) == key) && !product_names.contains(&p.name) {
            product_names.push(p.name.clone());
        }
    }
    let mut added: Vec<String> = Vec::new();
    for r in &new_rows {
        if !added.contains(&r.item) {
            added.push(r.item.clone());
        }
    }
    if !new_rows.is_empty() || !new_products.is_empty() {
        upsert(&mut rows, new_rows);
        upsert_products(&mut have, new_products);
        write_note(root, &rows, &have, today)?;
    } else {
        added.clear();
    }
    let report = Report { source, added, others, unassigned, oldest: oldest(&rows, today) };
    Ok((report, product_names))
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
    let old = fs::read_to_string(&note_path).map_err(|e| e.to_string())?;
    let mut rows = parse_note(&old);
    let mut have = parse_products(&old);
    let mut dropped: Vec<&str> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    for k in keys {
        let before = rows.len() + have.len();
        rows.retain(|r| !r.item.eq_ignore_ascii_case(k));
        have.retain(|p| food(&p.name) != Some(k));
        if rows.len() + have.len() < before {
            dropped.push(k);
        } else {
            missing.push(k);
        }
    }
    if !dropped.is_empty() {
        write_note(root, &rows, &have, today)?;
    }
    let mut reply = if dropped.is_empty() { String::new() } else { format!("Dropped {} from the pantry", dropped.join(", ")) };
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

/// Runs `f` with the products extractor: the helper at `DISCORD_INBOX_PRODUCTS` (absolute path), or none.
fn with_helper<T>(f: impl FnOnce(Option<&dyn Fn(&str) -> Option<String>>) -> T) -> T {
    match std::env::var("DISCORD_INBOX_PRODUCTS").ok().map(PathBuf::from).filter(|p| p.is_absolute()) {
        Some(helper) => {
            let ex = move |p: &str| ocr::run_args(&helper, &[], p.as_bytes(), Duration::from_secs(45));
            f(Some(&ex as &dyn Fn(&str) -> Option<String>))
        }
        None => f(None),
    }
}

/// Runs a command against the vault at `root`. `today` is the sender's local date.
pub fn run(root: &Path, cmd: Command, today: &str) -> Result<String, String> {
    with_helper(|ex| run_with(root, cmd, today, ex))
}

/// `run` with the products extractor given: tests pass a stub, `run` passes the helper or none (#159).
pub fn run_with(root: &Path, cmd: Command, today: &str, extract: Option<&dyn Fn(&str) -> Option<String>>) -> Result<String, String> {
    match cmd {
        Command::Restock { date, all } => {
            let d = date.unwrap_or_else(|| today.to_string());
            restock_with(root, &d, all, today, extract).map(|(r, names)| reply(&r, &names))
        }
        Command::OutOf(item) => out_of(root, &item, today),
    }
}

// ---- #159: bought products in the kitchen pantry note ----

/// A bought product from the kitchen pantry note's products block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub name: String,
    pub brand: String,
    pub size: String,
    pub bought: String,
}

pub const PRODUCTS_OPEN: &str = "<!-- pantry:products -->";
pub const PRODUCTS_CLOSE: &str = "<!-- /pantry:products -->";
pub const PRODUCT_DAYS: i64 = 90;
pub const MAX_PRODUCTS: usize = 60;

/// The extractor's instructions; the cleaned OCR text follows.
const EXTRACT_HEAD: &str = "You list the food and drink products on one store receipt or store-app purchase-history screen. Reply with these lines and nothing else:\ndate: <purchase date as YYYY-MM-DD, or unknown>\n- <product name> | <brand, or -> | <package size, or ->\nOne line per food or drink product. Write the name in full words (expand a receipt abbreviation only when you are sure). Size is the package size as printed (24 oz, 12 ct, 8 oz / 12 slices). Leave out prices, totals, quantities bought, order and card numbers, addresses and every person's name. Leave out anything that is not food or drink. No food: reply with the date line only.\n\nText:\n";

fn date_of(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn has_digit_run(s: &str, n: usize) -> bool {
    s.as_bytes().windows(n).any(|w| w.iter().all(u8::is_ascii_digit))
}

/// A unit word that makes the number before it a size ("1000 mL", "1.25 lb"), not a price or a card digit.
fn is_unit(w: &str) -> bool {
    matches!(
        w.trim_end_matches('.').to_ascii_lowercase().as_str(),
        "g" | "kg" | "mg" | "ml" | "cl" | "l" | "oz" | "lb" | "lbs" | "ct" | "pk" | "pc" | "pcs" | "ea" | "count" | "slice" | "slices"
    )
}

/// A number token sized by a unit: its own letters ("1000mL") or the next token ("1000 mL").
fn sized(t: &str, next: Option<&str>) -> bool {
    let letters = t.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ',');
    next.is_some_and(is_unit) || (letters.len() < t.len() && is_unit(letters))
}

/// A token that is a price, a card number or a `*` mask. Dropped whole. A 9+ digit run is a card, phone or order
/// number; a 4-digit group is a card's last four unless a unit sizes it. A two-decimal number is a price unless sized.
fn money_token(t: &str, next: Option<&str>) -> bool {
    if t.contains(['$', '€', '£', '¥', '*']) || has_digit_run(t, 9) {
        return true;
    }
    let sz = sized(t, next);
    if has_digit_run(t, 4) && !sz {
        return true;
    }
    !sz && t.contains('.') && t.chars().all(|c| c.is_ascii_digit() || c == '.') && t.rsplit('.').next().is_some_and(|f| f.len() == 2)
}

/// One product field, safe for the pantry note: no secrets, markup, prices or card digits; 60 chars at most.
pub fn clean_field(s: &str) -> String {
    let safe = redact::sensitive(s);
    let spaced: String = safe.chars().map(|c| if c.is_control() || "|·<>[]()#`".contains(c) { ' ' } else { c }).collect();
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let kept: Vec<&str> = words.iter().enumerate().filter(|(i, t)| !money_token(t, words.get(i + 1).copied())).map(|(_, t)| *t).collect();
    kept.join(" ").chars().take(60).collect::<String>().trim().to_string()
}

/// A size keeps only if a digit survives the cleaning ("24 oz" yes, "-" or "Costco" no).
fn clean_size(s: &str) -> String {
    let c = clean_field(s);
    if c.chars().any(|ch| ch.is_ascii_digit()) { c } else { String::new() }
}

/// A table or extractor cell: `-` is empty, anything else cleaned.
fn cell(s: &str) -> String {
    if s.trim() == "-" { String::new() } else { clean_field(s) }
}

fn product_line(p: &Product) -> Option<String> {
    date_of(&p.bought)?;
    let name = clean_field(&p.name);
    if name.is_empty() {
        return None;
    }
    let or_dash = |s: String| if s.is_empty() { "-".to_string() } else { s };
    Some(format!("- {name} · {} · {} · {}", or_dash(clean_field(&p.brand)), or_dash(clean_size(&p.size)), p.bought))
}

/// The products block's lines between its markers, as written. `-` reads as empty.
pub fn parse_products(text: &str) -> Vec<Product> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t == PRODUCTS_OPEN {
            inside = true;
            continue;
        }
        if t == PRODUCTS_CLOSE {
            break;
        }
        if !inside {
            continue;
        }
        let Some(body) = t.strip_prefix("- ") else { continue };
        let parts: Vec<&str> = body.split(" · ").collect();
        if parts.len() != 4 || date_of(parts[3].trim()).is_none() {
            continue;
        }
        let field = |s: &str| if s.trim() == "-" { String::new() } else { s.trim().to_string() };
        out.push(Product { name: field(parts[0]), brand: field(parts[1]), size: field(parts[2]), bought: parts[3].trim().to_string() });
    }
    out
}

/// What `restock` sends the extractor for one receipt: the instructions, then the cleaned OCR lines.
pub fn extract_prompt(note: &str) -> String {
    let lines: Vec<&str> = ocr_lines(note).into_iter().filter(|l| !is_tender_line(l)).collect();
    let text: String = redact::sensitive(&lines.join("\n")).chars().take(4000).collect();
    format!("{EXTRACT_HEAD}{text}")
}

/// The extractor's reply as products. `bought` is its `date:` when that lies within 60 days before the note's date.
pub fn parse_extract(reply: &str, note_date: &str) -> Vec<Product> {
    let Some(note) = date_of(note_date) else { return Vec::new() };
    let mut bought = note_date.to_string();
    if let Some(d) = reply.lines().find_map(|l| l.trim().strip_prefix("date:")) {
        if let Some(b) = date_of(d.trim()) {
            if (0..=60).contains(&(note - b).num_days()) {
                bought = b.format("%Y-%m-%d").to_string();
            }
        }
    }
    let mut out: Vec<Product> = Vec::new();
    for line in reply.lines() {
        let Some(body) = line.trim().strip_prefix("- ") else { continue };
        let cells: Vec<&str> = body.split('|').collect();
        let name = cell(cells[0]);
        if name.is_empty() || out.iter().any(|p| foods::norm(&p.name) == foods::norm(&name)) {
            continue;
        }
        let brand = cells.get(1).map(|c| cell(c)).unwrap_or_default();
        let size = cells.get(2).map(|c| clean_size(c)).unwrap_or_default();
        out.push(Product { name, brand, size, bought: bought.clone() });
        if out.len() == 40 {
            break;
        }
    }
    out
}

/// Same product (by normalised name) replaces brand, size and bought unless the new purchase is older.
/// An empty new brand or size keeps the old one. Oldest first, then by name.
pub fn upsert_products(have: &mut Vec<Product>, new: Vec<Product>) {
    for n in new {
        let key = foods::norm(&n.name);
        match have.iter_mut().find(|p| foods::norm(&p.name) == key) {
            Some(p) => {
                let older = matches!((date_of(&n.bought), date_of(&p.bought)), (Some(a), Some(b)) if a < b);
                if !older {
                    if !n.brand.is_empty() {
                        p.brand = n.brand;
                    }
                    if !n.size.is_empty() {
                        p.size = n.size;
                    }
                    p.bought = n.bought;
                }
            }
            None => have.push(n),
        }
    }
    have.sort_by(|a, b| (a.bought.as_str(), a.name.as_str()).cmp(&(b.bought.as_str(), b.name.as_str())));
}

/// The products the kitchen pantry note holds for `today`: a regular file only (links refused), at most
/// PRODUCT_DAYS old and at most a day ahead, every field cleaned again, newest first, then by name.
pub fn products_at(note: &Path, today: &str) -> Vec<Product> {
    let Some(t) = date_of(today) else { return Vec::new() };
    if !is_regular(note) {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(note) else { return Vec::new() };
    let mut out: Vec<Product> = parse_products(&text)
        .into_iter()
        .filter_map(|p| {
            let name = clean_field(&p.name);
            let age = (t - date_of(&p.bought)?).num_days();
            if name.is_empty() || age > PRODUCT_DAYS || age < -1 {
                return None;
            }
            Some(Product { name, brand: clean_field(&p.brand), size: clean_size(&p.size), bought: p.bought })
        })
        .collect();
    out.sort_by(|a, b| b.bought.cmp(&a.bought).then_with(|| a.name.cmp(&b.name)));
    out.truncate(MAX_PRODUCTS);
    out
}

/// Regular `<date> *.md` notes in the solo-receipt folder, as (title, text). Empty when the folder can't be read.
fn filed_receipts(root: &Path, date: &str) -> Vec<(String, String)> {
    let Ok(rd) = fs::read_dir(root.join("State Street")) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    paths.sort();
    let prefix = format!("{date} ");
    let mut out = Vec::new();
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(title) = name.strip_suffix(".md").and_then(|s| s.strip_prefix(&prefix)) else { continue };
        if !is_regular(&p) {
            continue;
        }
        out.push((title.to_string(), fs::read_to_string(&p).unwrap_or_default()));
    }
    out
}

/// A receipt whose OCR names Costco (spacing and 0-for-o forgiven).
fn costco_text(text: &str) -> bool {
    ocr_lines(text).iter().any(|l| {
        let m: String = l.to_lowercase().replace('0', "o").split_whitespace().collect();
        m.contains("costco")
    })
}
