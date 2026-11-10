//! #130 unit A: the meal reader. The model's JSON becomes items only through `enforce`/`parse`, so an impossible number never reaches the log. Weight precedence per food: a weight he typed beside that food (bound by `nutrition::typed_spans`) > a scale reading, only when an image is attached (` (scale)`) > the reader's estimate (` (estimated)`). A typed weight no food matches is listed under Not counted. Plain water and sparkling water are logged at 0 kcal. The helper is injected through `PIP_MODEL` (see pip.rs); this file never names a model or a network endpoint.
use crate::foods::{self, SavedFood};
use crate::nutrition::{self, Food, Item, Parsed, Unit, Unknown};
use crate::{ocr, redact};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Most items kept from one reply, and most unreadable strings kept.
const MAX_ITEMS: usize = 20;
const MAX_UNREADABLE: usize = 10;
/// Characters kept from one unreadable string.
const MAX_UNREADABLE_CHARS: usize = 80;
const IMPOSSIBLE: &str = "the reader gave an impossible number";
const UNPLACED: &str = "the reader could not place it";
const PAST_LIMIT: &str = "past the 20-item limit; not counted";
const UNBOUND: &str = "you typed this weight but no food the reader listed matches it; not counted";
const TIED: &str = "this typed weight fits more than one food; those lines are estimates";
const EACH: &str = "a weight typed with \"each\" is not split across foods; type a weight beside each food";
const TYPED_RANGE: &str = "you typed this weight; one food must be 1 g to 5 kg";
const NO_WEIGHT: &str = "the reader gave no weight";
const MORE: &str = "not counted";
/// The only foods logged at 0 kcal without a table row: plain water (ruling 6460436671: no coffee).
const ZERO_KCAL: &[&str] = &["water", "sparkling water"];
/// The prompt's example object. A reply that only echoes it is not the answer (see `is_echo`).
const EXAMPLE: &str = r#"{"items":[{"name":"cooked egg noodles","grams":100,"source":"text","kcal_100":138,"protein_100":4.5,"carbs_100":25,"fat_100":2.1,"veg":false}],"unreadable":[]}"#;
/// File extensions that count as an image on their own (the photo path keeps the attachment's extension).
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "avif"];
/// ISO-BMFF brands of HEIC, HEIF and AVIF stills.
const HEIF_BRANDS: &[&[u8]] = &[b"heic", b"heif", b"mif1", b"avif"];

/// What the helper gave back: a parsed reading, or nothing usable (helper failed, timed out, or printed non-JSON).
pub enum Reading {
    Read(Parsed),
    /// A printed Nutrition Facts panel (#146): saved as a food, never a meal.
    Label(Label),
    Down,
}

/// A saved food's label as the reader copied it (#146): the panel's per-serving numbers, `None` where it prints nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub name: String,
    pub serving_g: Option<f64>,
    pub serving_pc: Option<f64>,
    pub kcal: Option<f64>,
    pub protein: Option<f64>,
    pub carbs: Option<f64>,
    pub fat: Option<f64>,
}

/// The label reply: a `label` object with the panel's values. `None` for any other reply, or with no name.
pub fn parse_label(out: &str) -> Option<Label> {
    let v = reply_object(out)?;
    let l = v.get("label")?.as_object()?;
    let name = model_text(l.get("name")?.as_str()?);
    if name.is_empty() {
        return None;
    }
    let num = |k: &str| l.get(k).and_then(Value::as_f64);
    Some(Label {
        name,
        serving_g: num("serving_g"),
        serving_pc: num("serving_pc"),
        kcal: num("kcal"),
        protein: num("protein"),
        carbs: num("carbs"),
        fat: num("fat"),
    })
}

/// The prompt on stdin. Photos are named by path so the helper's Read tool can open them.
pub fn prompt(text: &str, photos: &[PathBuf]) -> String {
    prompt_saved(text, photos, &[])
}

/// `prompt`, plus a rule listing his saved foods (name and one serving), so the reader can mark an item as one of them.
pub fn prompt_saved(text: &str, photos: &[PathBuf], foods: &[SavedFood]) -> String {
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
    let mut lines = vec![
        "You read one food message for a calorie log. Reply with JSON only: no prose, no code fence.".to_string(),
        message,
        photo_line,
        "Rules:".to_string(),
        "- List every food eaten as one item: fix typos, split foods written together, name unfamiliar foods plainly (e.g. \"cooked egg noodles\").".to_string(),
        "- If the message gives a weight for a food (g, grams, kg, oz, lb), use exactly that weight and set \"source\": \"text\". Otherwise estimate grams from the photo or a normal portion and set \"source\": \"estimate\". Never change a typed weight.".to_string(),
        "- A photo of a kitchen scale display is a weight he measured: use the number shown and set \"source\": \"scale\". If one photo shows an empty container and a later one shows the same container with food, the food weighs the difference (subtract the empty reading). Name which food each scale photo is weighing from what is in the container.".to_string(),
        "- A receipt photo: list the food items bought as the meal at their printed quantities; skip non-food lines; never copy card numbers, totals or addresses.".to_string(),
        r#"- A photo of a packaged food's printed Nutrition Facts panel is a label, not a meal: reply {"label":{"name":"<product name from the package>","serving_g":140,"serving_pc":8,"kcal":230,"protein":9,"carbs":28,"fat":9},"items":[],"unreadable":[]}. Values are per serving, copied from the panel. A field the panel does not print is null; serving_pc is null when the serving is not a count of pieces. Never estimate a label field. A plate, bowl, receipt or scale display is never a label."#.to_string(),
        "- Plain water or sparkling water: name it exactly \"water\" or \"sparkling water\", with kcal_100 0.".to_string(),
        "- Per-100 g values are for the food as eaten (cooked), USDA-style.".to_string(),
        "- Anything you cannot turn into an item goes in \"unreadable\" with a short reason. No food at all: both lists empty.".to_string(),
        EXAMPLE.to_string(),
    ];
    if !foods.is_empty() {
        let list = foods.iter().map(|f| format!("{} ({})", f.name, foods::serving_text(f))).collect::<Vec<_>>().join("; ");
        lines.insert(6, format!("- Saved foods, from his own labels (name; one serving): {}. An item that is one of these foods gets \"saved\": its exact name from this list, and \"count\": the number of pieces he typed for it, copied as written (never count pieces in a photo; do no arithmetic); leave \"count\" out when he typed no number for it. Fill every other field as usual. The caption and a photo of the same food are one item, never two. List each saved food once, marked: never add a second, unmarked item for it, even when the photo shows it or you would name it differently. Never put \"saved\" on a food that is not in this list.", redact::sensitive(&list)));
    }
    lines.join("\n")
}

/// Runs the helper once: photo paths as arguments, the prompt on stdin. Down on any failure; the output is never printed.
pub fn read(cmd: &Path, text: &str, photos: &[PathBuf], timeout: Duration) -> Reading {
    read_saved(cmd, text, photos, &[], timeout)
}

/// `read`, telling the reader his saved foods so it can mark them (`parse_saved` prices the marks).
pub fn read_saved(cmd: &Path, text: &str, photos: &[PathBuf], foods: &[SavedFood], timeout: Duration) -> Reading {
    let args: Vec<String> = photos.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let Some(out) = ocr::run_args(cmd, &args, prompt_saved(text, photos, foods).as_bytes(), timeout) else {
        eprintln!("pip: model helper gave nothing");
        return Reading::Down;
    };
    if let Some(label) = parse_label(&out) {
        return Reading::Label(label);
    }
    match parse_saved(text, &out, foods) {
        Some(p) if !photos.iter().any(|f| is_image(f)) => Reading::Read(without_photo_scales(p)),
        Some(p) => Reading::Read(p),
        None => {
            eprintln!("pip: model output was not the JSON shape");
            Reading::Down
        }
    }
}

/// With no image attached, nothing was measured: a `scale` label is only the reader's word, so the line becomes an
/// estimate. A weight over the 3 kg text cap is reported as impossible, not logged.
fn without_photo_scales(mut p: Parsed) -> Parsed {
    let mut items = Vec::with_capacity(p.items.len());
    for mut item in std::mem::take(&mut p.items) {
        if !item.scale {
            items.push(item);
        } else if item.grams > 3000.0 {
            p.unknown.push(Unknown { text: model_text(&format!("{} {} g", item.food.name, item.grams)), hint: IMPOSSIBLE.to_string() });
        } else {
            item.scale = false;
            item.estimated = true;
            items.push(item);
        }
    }
    p.items = items;
    p
}

/// Whether an attached file is a real image: its first bytes are an image signature, or its name ends in an image
/// extension. Std only; a file that cannot be read is not an image.
pub fn is_image(path: &Path) -> bool {
    let by_name = path.extension().and_then(|e| e.to_str()).is_some_and(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()));
    if by_name {
        return true;
    }
    let mut head = Vec::new();
    let read = fs::File::open(path).and_then(|f| f.take(12).read_to_end(&mut head));
    read.is_ok() && has_image_magic(&head)
}

/// JPEG, PNG, GIF, WEBP and HEIF/AVIF signatures.
fn has_image_magic(b: &[u8]) -> bool {
    let riff_webp = b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP";
    let heif = b.len() >= 12 && &b[4..8] == b"ftyp" && HEIF_BRANDS.contains(&&b[8..12]);
    b.starts_with(&[0xFF, 0xD8, 0xFF]) || b.starts_with(&[0x89, 0x50, 0x4E, 0x47]) || b.starts_with(b"GIF8") || riff_webp || heif
}

/// Finds the JSON object in the reply (prose and code fences are skipped) and checks every item.
/// `None` when there is no object with an `items` array.
pub fn parse(text: &str, out: &str) -> Option<Parsed> {
    parse_saved(text, out, &[])
}

/// `parse`, plus his saved foods: an item the reader marked (`"saved"`, `"count"`) is priced from the label when the
/// mark is valid (`foods::price`); otherwise it stays the reader's estimate and the reply says why. With no foods the
/// marks are ignored.
pub fn parse_saved(text: &str, out: &str, foods: &[SavedFood]) -> Option<Parsed> {
    let v = reply_object(out)?;
    let items = v.get("items")?.as_array()?;

    // Each typed weight binds to at most one reader item; only the first MAX_ITEMS items are read.
    let spans = nutrition::typed_spans(text);
    let names: Vec<String> = items.iter().take(MAX_ITEMS).map(|raw| raw.get("name").and_then(Value::as_str).map(model_text).unwrap_or_default()).collect();
    let (binds, unbound) = bind(&spans, &names);

    // A number in a piece that names a saved food by an exact saved name belongs to that food (round 9).
    let counts: Vec<f64> = nutrition::split_pieces(text)
        .into_iter()
        .filter(|p| {
            let t = foods::take(p, foods, true);
            t.items.is_empty() && t.unknown.is_empty()
        })
        .flat_map(|p| nutrition::typed_counts(&p))
        .collect();
    let mut parsed = Parsed::default();
    let mut seen: Vec<String> = Vec::new();
    let mut seen_marks: Vec<String> = Vec::new();
    for (at, raw) in items.iter().enumerate() {
        if at >= MAX_ITEMS {
            // Past the cap: each of the next few is named as unknown, then one line counts the rest. Never silent.
            if at < MAX_ITEMS + MAX_UNREADABLE {
                let name = raw.get("name").and_then(Value::as_str).unwrap_or("(unnamed item)");
                parsed.unknown.push(Unknown { text: model_text(name), hint: PAST_LIMIT.to_string() });
            }
            continue;
        }
        let typed = binds[at].map(|j| spans[j].grams);
        let est = item_from(raw, typed);
        let mark = raw.get("saved").and_then(Value::as_str).map(model_text).filter(|m| !m.is_empty() && !foods.is_empty());
        match mark {
            None => match est {
                Ok(item) => parsed.items.push(item),
                Err(u) => parsed.unknown.push(with_typed(u, typed)),
            },
            Some(m) => {
                // Each saved food yields at most one line per reply, keyed on the food the mark resolves to.
                let dup = match foods::marked(&m, foods) {
                    Some(f) if seen.contains(&f.name) => true,
                    Some(f) => {
                        seen.push(f.name.clone());
                        false
                    }
                    None if seen_marks.contains(&foods::norm(&m)) => true,
                    None => {
                        seen_marks.push(foods::norm(&m));
                        false
                    }
                };
                if dup {
                    parsed.unknown.push(Unknown { text: format!("saved food \"{m}\""), hint: "marked twice by the reader; counted once".to_string() });
                    continue;
                }
                let count = raw.get("count").and_then(Value::as_f64);
                let scale = est.as_ref().ok().filter(|i| i.scale).map(|i| i.grams);
                match foods::price(text, &m, count, typed, scale, &counts, foods) {
                    Ok(item) => parsed.items.push(item),
                    // The mark is not usable: the item stays the reader's estimate, and the reply says why.
                    Err(why) => match est {
                        Ok(item) => {
                            parsed.unknown.push(Unknown { text: format!("saved food \"{m}\""), hint: format!("{why}; {} is counted as the reader's estimate", item.food.name) });
                            parsed.items.push(item);
                        }
                        Err(u) => parsed.unknown.push(with_typed(u, typed)),
                    },
                }
            }
        }
    }
    // A saved food he typed that no mark resolved to is named, never dropped silently. Code cannot tell whether the
    // reader listed it unmarked, so the line is true either way.
    if !foods.is_empty() {
        for f in foods.iter().filter(|f| !seen.iter().any(|n| *n == f.name)) {
            let found = nutrition::split_pieces(text).into_iter().find(|p| {
                let t = foods::take(p, std::slice::from_ref(f), true);
                !t.items.is_empty() || !t.unknown.is_empty()
            });
            if let Some(piece) = found {
                parsed.unknown.push(Unknown {
                    text: piece,
                    hint: format!("not priced from your saved label for {}: if a line above is this food, that is the reader's estimate; if not, it was not counted", f.name),
                });
            }
        }
    }
    parsed.unknown.extend(unbound);
    let rest = items.len().saturating_sub(MAX_ITEMS + MAX_UNREADABLE);
    if rest > 0 {
        parsed.unknown.push(Unknown { text: format!("and {rest} more not counted"), hint: PAST_LIMIT.to_string() });
    }
    if let Some(list) = v.get("unreadable").and_then(Value::as_array) {
        for s in list.iter().filter_map(Value::as_str).take(MAX_UNREADABLE) {
            parsed.unknown.push(Unknown { text: model_text(s), hint: UNPLACED.to_string() });
        }
        let extra = list.iter().filter(|s| s.is_string()).count().saturating_sub(MAX_UNREADABLE);
        if extra > 0 {
            parsed.unknown.push(Unknown { text: format!("and {extra} more unreadable"), hint: UNPLACED.to_string() });
        }
    }
    Some(parsed)
}

/// Whether a word of a reader name and a word of a typed span name the same food: equal, plural, one-letter typo,
/// or `ies`/`es` plurals (`berries`/`berry`, `tomatoes`/`tomato`).
fn same(a: &str, b: &str) -> bool {
    crate::recipes::word_match(a, b) || crate::recipes::word_match(&stem(a), &stem(b))
}

/// The singular a plural word is matched on: `ies` becomes `y`, a trailing `es` is dropped.
fn stem(w: &str) -> String {
    if let Some(s) = w.strip_suffix("ies") {
        return format!("{s}y");
    }
    w.strip_suffix("es").unwrap_or(w).to_string()
}

/// The words of a reader name that match the words of a span: how many of the name's words the span names.
/// A span with `each` names no food. Alias rule: when no word matches, a span that is exactly a table alias for
/// this name counts as all of the name's words (`lookup` sees only his words, never a model name).
fn hits(words: &[String], name: &str, span: &nutrition::TypedSpan) -> usize {
    if span.words.iter().any(|w| w == "each") {
        return 0;
    }
    let n = words.iter().filter(|a| span.words.iter().any(|b| same(a, b))).count();
    let alias = n == 0
        && matches!((nutrition::lookup(&span.words.join(" ")), nutrition::table_food(name)), (Ok(f), Some(g)) if std::ptr::eq(f, g));
    if alias {
        words.len().max(1)
    } else {
        n
    }
}

/// The typed span each reader name is bound to (`None` when none), and a Not-counted line for every span that no
/// name is bound to. A span with several claimants goes to the one with more hits, then fewer misses; a tie leaves
/// the span unbound and TIED.
fn bind(spans: &[nutrition::TypedSpan], names: &[String]) -> (Vec<Option<usize>>, Vec<Unknown>) {
    let words: Vec<Vec<String>> = names.iter().map(|n| nutrition::meal_words(n)).collect();
    let h: Vec<Vec<usize>> = (0..names.len()).map(|i| spans.iter().map(|s| hits(&words[i], &names[i], s)).collect()).collect();

    // Each name picks the span(s) it hits most; two equal tops are split by order when the spans read the same.
    let mut tops: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
    let mut pick: Vec<Option<usize>> = vec![None; names.len()];
    for i in 0..names.len() {
        let top = h[i].iter().copied().max().unwrap_or(0);
        if top > 0 {
            tops[i] = (0..spans.len()).filter(|&j| h[i][j] == top).collect();
        }
    }
    for i in 0..names.len() {
        let set = &tops[i];
        match set.len() {
            0 => {}
            1 => pick[i] = Some(set[0]),
            _ if set.iter().all(|&j| spans[j].words == spans[set[0]].words) => {
                let r = (0..i).filter(|&k| tops[k] == *set).count();
                pick[i] = set.get(r).copied();
            }
            _ => {}
        }
    }

    // Each span settles its claimants.
    let mut bound: Vec<Option<usize>> = vec![None; names.len()];
    let mut tied = vec![false; spans.len()];
    for j in 0..spans.len() {
        let claimants: Vec<usize> = (0..names.len()).filter(|&i| pick[i] == Some(j)).collect();
        if let [only] = claimants[..] {
            bound[only] = Some(j);
        } else if claimants.len() > 1 {
            let best_hits = claimants.iter().map(|&i| h[i][j]).max().unwrap_or(0);
            let most: Vec<usize> = claimants.iter().copied().filter(|&i| h[i][j] == best_hits).collect();
            let misses = |i: usize| words[i].len().saturating_sub(h[i][j]);
            let fewest = most.iter().map(|&i| misses(i)).min().unwrap_or(0);
            let winners: Vec<usize> = most.iter().copied().filter(|&i| misses(i) == fewest).collect();
            match winners[..] {
                [w] => bound[w] = Some(j),
                _ => tied[j] = true,
            }
        }
    }

    let mut unknown = Vec::new();
    for (j, span) in spans.iter().enumerate() {
        let hint = if span.words.iter().any(|w| w == "each") {
            EACH
        } else if tied[j] {
            TIED
        } else if bound.iter().any(|b| *b == Some(j)) {
            continue;
        } else {
            UNBOUND
        };
        unknown.push(Unknown { text: model_text(&format!("{:.0} g {}", span.grams, span.words.join(" "))), hint: hint.to_string() });
    }
    if unknown.len() > MAX_UNREADABLE {
        let more = unknown.len() - MAX_UNREADABLE;
        unknown.truncate(MAX_UNREADABLE);
        unknown.push(Unknown { text: format!("and {more} more typed weights"), hint: MORE.to_string() });
    }
    (bound, unknown)
}

/// The reply's JSON object that holds an `items` array. Prose and fences around it are skipped. When the reply holds
/// two or more such objects, an echo of the prompt's example is skipped and the last one left wins; a lone object
/// is taken as it is.
fn reply_object(out: &str) -> Option<Value> {
    let mut found: Vec<Value> = Vec::new();
    for (at, _) in out.match_indices('{') {
        let mut objects = serde_json::Deserializer::from_str(&out[at..]).into_iter::<Value>();
        if let Some(Ok(v)) = objects.next() {
            if v.get("items").is_some_and(Value::is_array) {
                found.push(v);
            }
        }
    }
    let kept: Vec<&Value> = if found.len() >= 2 { found.iter().filter(|v| !is_echo(v)).collect() } else { found.iter().collect() };
    kept.last().copied().or(found.last()).cloned()
}

/// The prompt's example, echoed back: one item, `cooked egg noodles` at 100 g (matched on food and grams, not the whole object).
fn is_echo(v: &Value) -> bool {
    v.get("items").and_then(Value::as_array).is_some_and(|items| {
        items.len() == 1 && items[0].get("name").and_then(Value::as_str) == Some("cooked egg noodles") && items[0].get("grams").and_then(Value::as_f64) == Some(100.0)
    })
}

/// One item object from the reply. Missing `typed`/`veg` are false; missing macros are 0. `typed` is the weight he
/// typed beside this food, if `parse` bound one.
fn item_from(raw: &Value, typed: Option<f64>) -> Result<Item, Unknown> {
    let no_name = || Unknown { text: "(unnamed item)".to_string(), hint: "the reader gave no usable name".to_string() };
    let name = raw.get("name").and_then(Value::as_str).ok_or_else(no_name)?;
    let grams = match (raw.get("grams").and_then(Value::as_f64), typed) {
        (Some(g), _) => g,
        // `check` logs his typed weight and ignores the reader's grams, so a reply without them still counts.
        (None, Some(t)) => t,
        (None, None) => {
            let text = model_text(name);
            return Err(if text.is_empty() { no_name() } else { Unknown { text, hint: NO_WEIGHT.to_string() } });
        }
    };
    let kcal = raw.get("kcal_100").and_then(Value::as_f64).ok_or_else(no_name)?;
    let veg = raw.get("veg").and_then(Value::as_bool).unwrap_or(false);
    let macro_of = |key: &str| raw.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let source = match raw.get("source").and_then(Value::as_str) {
        Some("text") => Source::Text,
        Some("scale") => Source::Scale,
        Some(_) => Source::Estimate,
        // Older replies carry only the `typed` flag.
        None if raw.get("typed").and_then(Value::as_bool).unwrap_or(false) => Source::Text,
        None => Source::Estimate,
    };
    check(name, grams, source, typed, kcal, macro_of("protein_100"), macro_of("carbs_100"), macro_of("fat_100"), veg)
}

/// Where the reader says a weight came from: his typed text, a kitchen scale in a photo, or its own estimate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Text,
    Scale,
    Estimate,
}

/// Largest weight a scale reading or a typed weight may give (grams). The reader's own estimates keep the 3 kg cap.
const MAX_SCALE_G: f64 = 5000.0;

/// Checks one reader item and builds it. Weight precedence per food: a weight he typed beside it (`typed`) is logged
/// as typed, whatever the reader or a photo says; else a `Scale` reading is measured (` (scale)`, 5 kg cap); else the
/// reader's number is its estimate (` (estimated)`). A non-table food with no calories is unknown, except plain water.
#[allow(clippy::too_many_arguments)]
fn check(name: &str, grams: f64, source: Source, typed: Option<f64>, kcal_100: f64, p: f64, c: f64, f: f64, veg: bool) -> Result<Item, Unknown> {
    // The name is the model's text: redacted and flattened to one line before anything else sees it.
    let name = model_text(name);
    if name.is_empty() || name.chars().count() > 60 {
        return Err(Unknown { text: name, hint: "the reader gave no usable name".to_string() });
    }
    if let Some(t) = typed {
        if !(1.0..=MAX_SCALE_G).contains(&t) {
            return Err(Unknown { text: model_text(&format!("{name} {t} g")), hint: TYPED_RANGE.to_string() });
        }
    }
    let (grams, estimated, scale, cap) = match (typed, source) {
        (Some(g), _) => (g, false, false, MAX_SCALE_G),
        (None, Source::Scale) => (grams, false, true, MAX_SCALE_G),
        (None, _) => (grams, true, false, 3000.0),
    };
    // The model's numbers are checked even when a table name will override them. Limit 1000 kcal/100 g (pure fat is ~900).
    let table = nutrition::table_food(&name);
    let in_range = (1.0..=cap).contains(&grams)
        && (0.0..=1000.0).contains(&kcal_100)
        && (0.0..=100.0).contains(&p)
        && (0.0..=100.0).contains(&c)
        && (0.0..=100.0).contains(&f);
    if !in_range {
        return Err(Unknown { text: model_text(&format!("{name} {grams} g")), hint: IMPOSSIBLE.to_string() });
    }
    let food: &'static Food = match table {
        Some(food) => food,
        None if kcal_100 < 1.0 && !ZERO_KCAL.contains(&name.to_lowercase().as_str()) => {
            return Err(Unknown { text: name, hint: "the reader gave no calories".to_string() });
        }
        None => Food::model(&name, kcal_100, p, c, f, veg),
    };
    Ok(Item { food, grams, unit: Unit::G, qty: grams, assumed: false, estimated, scale })
}

/// The `enforce` entry point: one name and its weight. It binds the name to his typed spans the same way `parse` does.
/// Keeps its signature: the plan and breaker tests call it.
#[allow(clippy::too_many_arguments)]
pub fn enforce(text: &str, name: &str, grams: f64, source: Source, kcal_100: f64, p: f64, c: f64, f: f64, veg: bool) -> Result<Item, Unknown> {
    let spans = nutrition::typed_spans(text);
    let (binds, _) = bind(&spans, &[model_text(name)]);
    let typed = binds[0].map(|j| spans[j].grams);
    check(name, grams, source, typed, kcal_100, p, c, f, veg).map_err(|u| with_typed(u, typed))
}

/// A bound item that is rejected still shows his typed weight: the weight goes in front of the rejection's text,
/// unless the text already has it. The weight comes first, so `model_text`'s length cap cannot cut it off.
fn with_typed(u: Unknown, typed: Option<f64>) -> Unknown {
    match typed {
        Some(g) if !names_weight(&u.text, g) => Unknown { text: model_text(&format!("{g} g {}", u.text)), hint: u.hint },
        _ => u,
    }
}

/// True when `text` already shows the typed weight `g` as its own number, not as the tail of a longer one (`150 g` does
/// not name `50 g`).
fn names_weight(text: &str, g: f64) -> bool {
    let w = format!("{g} g");
    text.match_indices(&w).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + w.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_digit() || c == '.') && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Redacts first, then flattens to one line and caps the length, so a card number is never cut into a visible half.
/// Every string the model supplies (names, unreadable reasons, impossible-number texts) goes through here.
fn model_text(s: &str) -> String {
    let clean = redact::sensitive(s).split_whitespace().collect::<Vec<_>>().join(" ");
    clean.chars().take(MAX_UNREADABLE_CHARS).collect()
}
