//! #130 unit A: the meal reader. The model's JSON becomes items only through `enforce`, so a typed weight is never replaced and an impossible number never reaches the log. The helper is injected through `PIP_MODEL` (see pip.rs); this file never names a model or a network endpoint.
use crate::nutrition::{self, Food, Item, Parsed, Unit, Unknown};
use crate::{ocr, redact};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Most items kept from one reply, and most unreadable strings kept.
const MAX_ITEMS: usize = 20;
const MAX_UNREADABLE: usize = 10;
/// Characters kept from one unreadable string.
const MAX_UNREADABLE_CHARS: usize = 80;
const IMPOSSIBLE: &str = "the reader gave an impossible number";
const UNPLACED: &str = "the reader could not place it";

/// What the helper gave back: a parsed reading, or nothing usable (helper failed, timed out, or printed non-JSON).
pub enum Reading {
    Read(Parsed),
    Down,
}

/// The prompt on stdin. Photos are named by path so the helper's Read tool can open them.
pub fn prompt(text: &str, photos: &[PathBuf]) -> String {
    let message = if text.trim().is_empty() {
        "Message: (no text)".to_string()
    } else {
        format!("Message: \"{}\"", redact::sensitive(text))
    };
    let photo_line = if photos.is_empty() {
        "Photos: none".to_string()
    } else {
        let paths: Vec<String> = photos.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        format!("Photos: {} attached; read each with the Read tool: {}", photos.len(), paths.join(", "))
    };
    let lines = [
        "You read one food message for a calorie log. Reply with JSON only: no prose, no code fence.".to_string(),
        message,
        photo_line,
        "Rules:".to_string(),
        "- List every food eaten as one item: fix typos, split foods written together, name unfamiliar foods plainly (e.g. \"cooked egg noodles\").".to_string(),
        "- If the message gives a weight for a food (g, grams, kg, oz, lb), use exactly that weight and set \"typed\": true. Otherwise estimate grams from the photo or a normal portion and set \"typed\": false. Never change a typed weight.".to_string(),
        "- A receipt photo: list the food items bought as the meal at their printed quantities; skip non-food lines; never copy card numbers, totals or addresses.".to_string(),
        "- Per-100 g values are for the food as eaten (cooked), USDA-style.".to_string(),
        "- Anything you cannot turn into an item goes in \"unreadable\" with a short reason. No food at all: both lists empty.".to_string(),
        "{\"items\":[{\"name\":\"cooked egg noodles\",\"grams\":100,\"typed\":true,\"kcal_100\":138,\"protein_100\":4.5,\"carbs_100\":25,\"fat_100\":2.1,\"veg\":false}],\"unreadable\":[]}".to_string(),
    ];
    lines.join("\n")
}

/// Runs the helper once: photo paths as arguments, the prompt on stdin. Down on any failure; the output is never printed.
pub fn read(cmd: &Path, text: &str, photos: &[PathBuf], timeout: Duration) -> Reading {
    let args: Vec<String> = photos.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let Some(out) = ocr::run_args(cmd, &args, prompt(text, photos).as_bytes(), timeout) else {
        eprintln!("pip: model helper gave nothing");
        return Reading::Down;
    };
    match parse(text, &out) {
        Some(p) => Reading::Read(p),
        None => {
            eprintln!("pip: model output was not the JSON shape");
            Reading::Down
        }
    }
}

/// Finds the JSON object in the reply (prose and code fences are skipped) and checks every item.
/// `None` when there is no object with an `items` array.
pub fn parse(text: &str, out: &str) -> Option<Parsed> {
    let start = out.find('{')?;
    let end = out.rfind('}')?;
    if end < start {
        return None;
    }
    let v: Value = serde_json::from_str(&out[start..=end]).ok()?;
    let items = v.get("items")?.as_array()?;

    let mut parsed = Parsed::default();
    for raw in items.iter().take(MAX_ITEMS) {
        match item_from(text, raw) {
            Ok(item) => parsed.items.push(item),
            Err(u) => parsed.unknown.push(u),
        }
    }
    if let Some(list) = v.get("unreadable").and_then(Value::as_array) {
        for s in list.iter().filter_map(Value::as_str).take(MAX_UNREADABLE) {
            parsed.unknown.push(Unknown { text: unreadable_text(s), hint: UNPLACED.to_string() });
        }
    }
    Some(parsed)
}

/// One item object from the reply. Missing `typed`/`veg` are false; missing macros are 0.
fn item_from(text: &str, raw: &Value) -> Result<Item, Unknown> {
    let no_name = || Unknown { text: "(unnamed item)".to_string(), hint: "the reader gave no usable name".to_string() };
    let name = raw.get("name").and_then(Value::as_str).ok_or_else(no_name)?;
    let grams = raw.get("grams").and_then(Value::as_f64).ok_or_else(no_name)?;
    let kcal = raw.get("kcal_100").and_then(Value::as_f64).ok_or_else(no_name)?;
    let typed = raw.get("typed").and_then(Value::as_bool).unwrap_or(false);
    let veg = raw.get("veg").and_then(Value::as_bool).unwrap_or(false);
    let macro_of = |key: &str| raw.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    enforce(text, name, grams, typed, kcal, macro_of("protein_100"), macro_of("carbs_100"), macro_of("fat_100"), veg)
}

/// Checks one reader item and builds it. A name that is exactly a table name or alias uses the table's numbers.
/// A typed weight stays typed only when his text has that mass amount; otherwise the line is `estimated`.
#[allow(clippy::too_many_arguments)]
pub fn enforce(text: &str, name: &str, grams: f64, typed: bool, kcal_100: f64, p: f64, c: f64, f: f64, veg: bool) -> Result<Item, Unknown> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 || name.contains(['\n', '\r']) {
        return Err(Unknown { text: name.to_string(), hint: "the reader gave no usable name".to_string() });
    }
    // The model's numbers are checked even when a table name will override them. Limit 1000 kcal/100 g (pure fat is ~900).
    let table = nutrition::table_food(name);
    let in_range = (1.0..=3000.0).contains(&grams)
        && (0.0..=1000.0).contains(&kcal_100)
        && (0.0..=100.0).contains(&p)
        && (0.0..=100.0).contains(&c)
        && (0.0..=100.0).contains(&f);
    if !in_range {
        return Err(Unknown { text: format!("{name} {grams} g"), hint: IMPOSSIBLE.to_string() });
    }
    let food: &'static Food = match table {
        Some(food) => food,
        None => Food::model(name, kcal_100, p, c, f, veg),
    };
    let matches_typed = nutrition::typed_grams(text).iter().any(|t| (grams - t).abs() <= 0.5 + 0.01 * t);
    let estimated = !(typed && matches_typed);
    Ok(Item { food, grams, unit: Unit::G, qty: grams, assumed: false, estimated })
}

/// Redacts first, then flattens to one line and caps the length, so a card number is never cut into a visible half.
fn unreadable_text(s: &str) -> String {
    let clean = redact::sensitive(s).split_whitespace().collect::<Vec<_>>().join(" ");
    clean.chars().take(MAX_UNREADABLE_CHARS).collect()
}
