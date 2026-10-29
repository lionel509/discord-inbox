//! #146: saved foods. One note per packaged food in `PIP_FOODS_DIR` (the kitchen vault's Pantry/Foods), written by Pip from
//! a label photo: the label's per-serving numbers, and what he counts (pieces, grams or servings). Pip owns only the
//! block between its markers and the `Foods.md` index; the rest of a note is his. `take` turns a typed count into items
//! before the table and the reader see the text, so a saved food beats the table only when he names it in full.
use crate::nutrition::{self, Food, Item, Kind, Unit, Unknown};
use crate::pip_model::Label;
use crate::recipes;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const BLOCK_OPEN: &str = "<!-- pip:food -->";
pub const BLOCK_CLOSE: &str = "<!-- /pip:food -->";
pub const INDEX: &str = "Foods.md";
/// Most grams one typed count may make of a saved food (the same cap as a typed weight).
const MAX_G: f64 = 5000.0;

/// One saved food, read from its note (`load`) or built from a label (`from_label`).
#[derive(Clone, Debug, PartialEq)]
pub struct SavedFood {
    /// The note's file stem, the name Pip writes.
    pub name: String,
    /// Every name the food answers to: the stem, the H1 and the front-matter aliases.
    pub names: Vec<String>,
    pub serving_g: Option<f64>,
    pub serving_pc: Option<f64>,
    pub kcal: Option<f64>,
    pub protein: Option<f64>,
    pub carbs: Option<f64>,
    pub fat: Option<f64>,
    /// The last count he logged for this food (the default for a bare name).
    pub last: Option<(f64, Unit)>,
    pub saved: String,
}

/// Turns a reader's label into a saved food. Refuses a name it cannot file and any number out of range.
pub fn from_label(l: &Label, day: &str) -> Result<SavedFood, String> {
    let name = file_name(&l.name);
    if name.is_empty() {
        return Err("I couldn't read the product name; send the label with the front of the package".to_string());
    }
    let bad = |field: &str| format!("the reader gave an impossible number for {field}; send the label again");
    let ok = |v: Option<f64>, lo: f64, hi: f64| v.is_none_or(|x| (lo..=hi).contains(&x));
    if !ok(l.serving_g, 1.0, 2000.0) {
        return Err(bad("the serving"));
    }
    if !ok(l.serving_pc, 1.0, 200.0) {
        return Err(bad("the piece count"));
    }
    if !ok(l.kcal, 0.0, 2000.0) {
        return Err(bad("kcal"));
    }
    for (v, field) in [(l.protein, "protein"), (l.carbs, "carbs"), (l.fat, "fat")] {
        if !ok(v, 0.0, 200.0) {
            return Err(bad(field));
        }
    }
    // Per 100 g the label cannot be more than pure fat (about 900 kcal) or 100 g of any macro.
    if let Some(g) = l.serving_g {
        if l.kcal.is_some_and(|k| k * 100.0 / g > 1000.0) || [l.protein, l.carbs, l.fat].iter().flatten().any(|m| m * 100.0 / g > 100.0) {
            return Err(bad("the serving"));
        }
    }
    Ok(SavedFood {
        name: name.clone(),
        names: vec![name],
        serving_g: l.serving_g,
        serving_pc: l.serving_pc,
        kcal: l.kcal,
        protein: l.protein,
        carbs: l.carbs,
        fat: l.fat,
        last: None,
        saved: day.to_string(),
    })
}

/// A file name from a product name: separators and punctuation become spaces, runs collapse, 60 characters at most.
fn file_name(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if "/\\:*?\"<>|[]#^".contains(c) { ' ' } else { c }).collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = joined.trim_end_matches('.').chars().take(60).collect();
    short.trim().to_string()
}

/// The label values he has not got yet, in the order the note shows them. A missing piece count is not missing.
pub fn missing(f: &SavedFood) -> Vec<&'static str> {
    let mut out = Vec::new();
    if f.serving_g.is_none() {
        out.push("serving");
    }
    if f.kcal.is_none() {
        out.push("kcal");
    }
    if f.protein.is_none() {
        out.push("protein");
    }
    if f.carbs.is_none() {
        out.push("carbs");
    }
    if f.fat.is_none() {
        out.push("fat");
    }
    out
}

/// A number with no decimal when it is whole, else one decimal place.
fn num(x: f64) -> String {
    if (x - x.round()).abs() < 1e-9 {
        format!("{}", x.round() as i64)
    } else {
        format!("{x:.1}")
    }
}

fn opt(x: Option<f64>) -> String {
    x.map(num).unwrap_or_else(|| "?".to_string())
}

fn unit_word(q: f64, u: Unit) -> String {
    match u {
        Unit::Piece => format!("{} pc", num(q)),
        Unit::Portion if q == 1.0 => "1 serving".to_string(),
        Unit::Portion => format!("{} servings", num(q)),
        _ => format!("{} g", num(q)),
    }
}

/// The Pip-owned block of a note: the markers and the lines between them. No trailing newline.
pub fn format_block(f: &SavedFood) -> String {
    let serving_g = f.serving_g.map(|g| format!("{} g", num(g))).unwrap_or_else(|| "? g".to_string());
    let serving = match f.serving_pc {
        Some(pc) => format!("{serving_g} · {} pc", num(pc)),
        None => serving_g,
    };
    let mut lines = vec![
        BLOCK_OPEN.to_string(),
        format!("- serving: {serving}"),
        format!(
            "- per serving: {} kcal · P {} g · C {} g · F {} g",
            opt(f.kcal),
            opt(f.protein),
            opt(f.carbs),
            opt(f.fat)
        ),
    ];
    let miss = missing(f);
    if !miss.is_empty() {
        lines.push(format!("- missing: {}", miss.join(", ")));
    }
    if let Some((q, u)) = f.last {
        lines.push(format!("- last: {}", unit_word(q, u)));
    }
    lines.push(format!("- saved: {} from a label photo", f.saved));
    lines.push(BLOCK_CLOSE.to_string());
    lines.join("\n")
}

/// The first number in a value like `230 kcal`, `P 9 g` or `? kcal` (`None`).
fn first_number(s: &str) -> Option<f64> {
    s.split_whitespace().find_map(|w| w.parse::<f64>().ok())
}

/// Reads the block back. The name and aliases are not in the block, so they come back empty; `load` fills them.
pub fn parse_block(text: &str) -> Option<SavedFood> {
    let open = text.find(BLOCK_OPEN)?;
    let close = text[open..].find(BLOCK_CLOSE)? + open;
    let body = &text[open + BLOCK_OPEN.len()..close];
    let (mut serving_g, mut serving_pc, mut kcal, mut protein, mut carbs, mut fat) = (None, None, None, None, None, None);
    let (mut last, mut saved) = (None, String::new());
    for line in body.lines().map(str::trim).filter(|l| l.starts_with("- ")) {
        let line = &line[2..];
        if let Some(v) = line.strip_prefix("serving: ") {
            let mut parts = v.split(" · ");
            serving_g = parts.next().and_then(first_number);
            serving_pc = parts.next().and_then(first_number);
        } else if let Some(v) = line.strip_prefix("per serving: ") {
            let mut parts = v.split(" · ");
            kcal = parts.next().and_then(first_number);
            protein = parts.next().and_then(first_number);
            carbs = parts.next().and_then(first_number);
            fat = parts.next().and_then(first_number);
        } else if let Some(v) = line.strip_prefix("last: ") {
            let q = first_number(v)?;
            let u = if v.contains(" pc") {
                Unit::Piece
            } else if v.contains(" g") {
                Unit::G
            } else {
                Unit::Portion
            };
            last = Some((q, u));
        } else if let Some(v) = line.strip_prefix("saved: ") {
            saved = v.split_whitespace().next().unwrap_or_default().to_string();
        }
    }
    Some(SavedFood { name: String::new(), names: Vec::new(), serving_g, serving_pc, kcal, protein, carbs, fat, last, saved })
}

/// Lowercase words with every other character as a break, so `Northwind-Pork` and `northwind pork` compare equal.
fn norm(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>().join(" ")
}

/// Every foods note in the folder, sorted by name. The index and any note with no block are skipped.
pub fn load(dir: &Path) -> Vec<SavedFood> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<SavedFood> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md")))
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            if stem == "Foods" {
                return None;
            }
            let text = fs::read_to_string(&p).ok()?;
            let mut f = parse_block(&text)?;
            let (fm, body) = recipes::split_frontmatter(&text);
            let mut names = vec![stem.clone()];
            if let Some(h1) = body.lines().map(str::trim).find_map(|l| l.strip_prefix("# ")) {
                names.push(h1.trim().to_string());
            }
            names.extend(recipes::frontmatter_aliases(fm));
            f.name = stem;
            f.names = names;
            Some(f)
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The existing note for this name (any spelling `norm` makes equal), not the index.
fn find_note(dir: &Path, name: &str) -> Option<PathBuf> {
    let want = norm(name);
    fs::read_dir(dir).ok()?.filter_map(|e| e.ok().map(|e| e.path())).find(|p| {
        p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md"))
            && p.file_stem().is_some_and(|s| { let stem = s.to_string_lossy(); stem != "Foods" && norm(&stem) == want })
    })
}

/// Writes through a `.tmp` file and a rename, so a crash never leaves half a note.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_file_name(format!("{}.tmp", path.file_name().unwrap_or_default().to_string_lossy()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// The block replaces the old one in place; without one it goes before the hub line, or at the end.
fn splice(old: &str, block: &str) -> String {
    if let Some(open) = old.find(BLOCK_OPEN) {
        if let Some(close) = old[open..].find(BLOCK_CLOSE) {
            let end = open + close + BLOCK_CLOSE.len();
            return format!("{}{block}{}", &old[..open], &old[end..]);
        }
    }
    match old.find("**Hub:**") {
        Some(at) => format!("{}{block}\n\n{}", &old[..at], &old[at..]),
        None => format!("{}\n\n{block}\n", old.trim_end()),
    }
}

/// A new note: the front matter a Fidelity ingredient note carries, the block, and the hub line.
fn new_note(f: &SavedFood, block: &str) -> String {
    format!(
        "---\ntags:\n  - making\n  - ingredient\n  - packaged\nstatus: active\n---\n\n# {}\n\n{block}\n\nSaved by Pip from a label photo. Used in: (link recipes here)\n\n**Hub:** [[Home]] · [[Pantry]] · [[Foods]]\n",
        f.name
    )
}

/// The index: one line per saved food, rewritten whole from the notes.
fn write_index(dir: &Path) -> io::Result<()> {
    let mut lines = Vec::new();
    for f in load(dir) {
        let serving = match (f.serving_pc, f.serving_g) {
            (Some(pc), Some(g)) => format!("{} pc ({} g)", num(pc), num(g)),
            (None, Some(g)) => format!("{} g", num(g)),
            _ => "?".to_string(),
        };
        lines.push(format!("- [[{}]] — {} kcal per {serving}", f.name, opt(f.kcal)));
    }
    let text = format!("---\ntags:\n  - making\n  - index\n---\n\n# Saved Foods\n\n{}\n\n**Hub:** [[Home]] · [[Pantry]]\n", lines.join("\n"));
    write_atomic(&dir.join(INDEX), &text)
}

/// Writes the food's note (a new one, or its block in the note already there) and rewrites the index. Same name, same
/// note: a replay or a second photo updates in place and never adds a second note.
pub fn upsert(dir: &Path, f: &SavedFood) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let target = find_note(dir, &f.name).unwrap_or_else(|| dir.join(format!("{}.md", f.name)));
    let block = format_block(f);
    let text = match fs::read_to_string(&target) {
        Ok(old) => splice(&old, &block),
        Err(e) if e.kind() == io::ErrorKind::NotFound => new_note(f, &block),
        Err(e) => return Err(e),
    };
    write_atomic(&target, &text)?;
    write_index(dir)?;
    Ok(target)
}

/// Records the count he logged for a saved food, as its default. A no-op when it is already the last count.
pub fn remember(dir: &Path, name: &str, qty: f64, unit: Unit) -> io::Result<()> {
    let Some(mut f) = load(dir).into_iter().find(|f| f.name == name) else { return Ok(()) };
    if f.last == Some((qty, unit)) {
        return Ok(());
    }
    f.last = Some((qty, unit));
    upsert(dir, &f).map(|_| ())
}

/// The table-shaped food for a saved one, built once per distinct label (the cache), so a message leaks nothing new.
/// `None` when the note lacks the serving or the kcal, the two values a count cannot be worked out without.
pub fn food_of(f: &SavedFood) -> Option<&'static Food> {
    let sg = f.serving_g?;
    let kcal = f.kcal?;
    let key = format!("{}|{sg}|{kcal}|{:?}|{:?}|{:?}|{:?}|{:?}", f.name, f.serving_pc, f.protein, f.carbs, f.fat, sg);
    static CACHE: OnceLock<Mutex<HashMap<String, &'static Food>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = cache.lock().ok()?;
    if let Some(food) = map.get(&key) {
        return Some(food);
    }
    let per = |v: Option<f64>| v.unwrap_or(0.0) * 100.0 / sg;
    let food: &'static Food = Box::leak(Box::new(Food {
        name: Box::leak(f.name.clone().into_boxed_str()),
        aliases: &[],
        kcal: kcal * 100.0 / sg,
        protein: per(f.protein),
        carbs: per(f.carbs),
        fat: per(f.fat),
        veg: false,
        cup_g: None,
        piece_g: f.serving_pc.map(|pc| sg / pc),
        portion_g: sg,
        source: "label",
    }));
    map.insert(key, food);
    Some(food)
}

/// What `take` made of the text: items from saved foods, refusals, and the text it did not use.
pub struct Taken {
    pub items: Vec<Item>,
    pub unknown: Vec<Unknown>,
    pub rest: String,
}

/// Lowercase food words of a piece with its amounts removed; stop words dropped.
fn food_words(sc: &nutrition::Scan) -> Vec<String> {
    let mut kept = String::new();
    let mut at = 0;
    for a in &sc.amounts {
        if a.start >= at {
            kept.push_str(&sc.text[at..a.start]);
            kept.push(' ');
            at = a.end;
        }
    }
    kept.push_str(&sc.text[at..]);
    kept.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .filter(|w| !nutrition::STOP.contains(&w.as_str()))
        .collect()
}

/// Plural-tolerant equality of two lowercase words.
fn same(a: &str, b: &str) -> bool {
    a == b || a.strip_suffix('s') == Some(b) || b.strip_suffix('s') == Some(a)
}

/// Whether the food words name this food. A table name in the words (`salmon`) needs the food's whole name, so a saved
/// "Northwind Salmon Fillets" never takes `salmon 150g` off the table.
fn fits(words: &[String], f: &SavedFood) -> bool {
    f.names.iter().any(|name| {
        let nw = name_words(name);
        if nw.is_empty() || words.is_empty() {
            return false;
        }
        if nutrition::lookup(&words.join(" ")).is_ok() {
            nw.iter().all(|n| words.iter().any(|w| same(w, n)))
        } else {
            words.iter().all(|w| nw.iter().any(|n| same(w, n)))
        }
    })
}

fn name_words(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// Grams for a count of a saved food, or the reason it cannot be worked out.
fn grams_for(f: &SavedFood, q: f64, u: Unit) -> Result<f64, String> {
    let sg = f.serving_g.unwrap_or(0.0);
    match u {
        Unit::Piece => match f.serving_pc {
            Some(pc) => Ok(q * sg / pc),
            None => Err("its label has no piece count; say grams (wontons 200g) or servings (2 servings wontons)".to_string()),
        },
        Unit::Portion => Ok(q * sg),
        _ => Ok(q),
    }
}

/// Turns the saved foods' pieces of `text` into items. Each piece is a food name with at most one amount: a count
/// of pieces (`24 wontons`), grams (`wontons 200g`) or servings (`3 servings wontons`). A bare name takes the last
/// count he logged, else one serving, when `default_portion` (a message, not a recipe line). Everything else is left
/// in `rest` for the table and the reader; with no match `rest` is `text` itself.
pub fn take(text: &str, foods: &[SavedFood], default_portion: bool) -> Taken {
    let mut out = Taken { items: Vec::new(), unknown: Vec::new(), rest: text.to_string() };
    if foods.is_empty() {
        return out;
    }
    let mut kept: Vec<String> = Vec::new();
    let mut used = false;
    for piece in nutrition::split_pieces(text).iter().flat_map(|p| p.split(" with ")) {
        let cleaned = piece.replace("[[", "").replace("]]", "");
        let target = cleaned.split('|').next().unwrap_or_default().trim().to_string();
        let sc = nutrition::scan(&target);
        let words = food_words(&sc);
        let hits: Vec<&SavedFood> = foods.iter().filter(|f| fits(&words, f)).collect();
        match hits.as_slice() {
            [] => kept.push(piece.trim().to_string()),
            [_, _, ..] => {
                used = true;
                let names: Vec<&str> = hits.iter().map(|f| f.name.as_str()).collect();
                out.unknown.push(Unknown { text: piece.trim().to_string(), hint: format!("more than one saved food fits: {}; name one", names.join(", ")) });
            }
            [f] => {
                used = true;
                take_one(f, piece.trim(), &sc, default_portion, &mut out, &mut kept);
            }
        }
    }
    if used {
        out.rest = kept.join(", ");
    }
    out
}

/// One matched piece: its amount (or the default) worked out against the label, or refused with the reason.
fn take_one(f: &SavedFood, piece: &str, sc: &nutrition::Scan, default_portion: bool, out: &mut Taken, kept: &mut Vec<String>) {
    let refuse = |out: &mut Taken, hint: String| out.unknown.push(Unknown { text: piece.to_string(), hint });
    let Some(food) = food_of(f) else {
        return refuse(out, format!("its label note is missing {}; edit the note or send the label again", missing(f).join(", ")));
    };
    if sc.amounts.len() > 1 {
        return refuse(out, "two amounts in one line; send one".to_string());
    }
    if let Some(a) = sc.amounts.first() {
        match a.kind {
            Kind::Negative => return refuse(out, "negative amount; say how much you had".to_string()),
            Kind::Unreadable => return refuse(out, "amount not readable; write it as 100, 1/2 or ½".to_string()),
            Kind::Range => return refuse(out, "a range; send one number".to_string()),
            _ => {}
        }
    }
    let (q, u, assumed) = match sc.amounts.first() {
        None if !default_portion => return kept.push(piece.to_string()),
        None => match f.last {
            Some((q, u)) => (q, u, true),
            None => (1.0, Unit::Portion, true),
        },
        Some(a) => match a.kind {
            Kind::Mass => (a.value, Unit::G, false),
            Kind::Piece { .. } | Kind::Bare => (a.value, Unit::Piece, false),
            Kind::Serving | Kind::Multiplier => (a.value, Unit::Portion, false),
            _ => return refuse(out, "say pieces, grams or servings for this food (wontons 200g, 2 servings wontons)".to_string()),
        },
    };
    let grams = match grams_for(f, q, u) {
        Ok(g) => g,
        Err(hint) => return refuse(out, hint),
    };
    if !(1.0..=MAX_G).contains(&grams) {
        return refuse(out, "you typed this weight; one food must be 1 g to 5 kg".to_string());
    }
    out.items.push(Item { food, grams, unit: u, qty: q, assumed, estimated: false, scale: false });
}
