//! Read-only recipe notes to ingredient lines (U2). Recipes are only ever read;
//! the nutrition parser turns the lines into grams, and typed weights override them.
use crate::nutrition;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Recipe {
    pub name: String,
    pub names: Vec<String>,
    pub lines: Vec<String>,
    pub skipped: Vec<String>,
    /// Lines read from a range (`1–2 cups` as its midpoint), for the reply to mark as assumed.
    pub assumed: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Hit<'a> {
    pub recipe: &'a Recipe,
    pub servings: f64,
    pub rest: String,
    /// The message gave a count (`2 x`, `1/2 serving`, `half`), so it is used as is.
    pub explicit: bool,
    /// A typed weight of the recipe's main ingredient from a fuzzy hit (`honey garlic salmon 150g`).
    pub weight_g: Option<f64>,
    /// Found by the fuzzy pass (typos, two or more name words), not by an exact name.
    pub fuzzy: bool,
    /// The phrase the hit came from, shown back in the reply.
    pub matched: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Scaled {
    Count,
    Weight { food: &'static str, grams: f64 },
    Typed { food: &'static str, grams: f64 },
    Assumed,
}

/// Every `*.md` directly in `dir` that yields at least one ingredient line, sorted by name.
pub fn load(dir: &Path) -> Vec<Recipe> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Recipe> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.to_string_lossy().eq_ignore_ascii_case("md")))
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            let text = fs::read_to_string(&p).ok()?;
            parse_note(&stem, &text)
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn parse_note(stem: &str, text: &str) -> Option<Recipe> {
    let (fm, body) = split_frontmatter(text);
    let h1 = body
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim_start_matches(|c: char| !c.is_alphanumeric()).trim().to_string())
        .filter(|s| !s.is_empty());
    let mut names: Vec<String> = Vec::new();
    for n in std::iter::once(stem.to_string()).chain(h1).chain(frontmatter_aliases(fm)) {
        let n = n.trim().to_string();
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    }
    let (lines, skipped, assumed) = ingredient_lines(body);
    if lines.is_empty() {
        return None;
    }
    Some(Recipe { name: stem.to_string(), names, lines, skipped, assumed })
}

/// Exact name first; the fuzzy pass runs only when no exact name is in the text.
pub fn resolve<'a>(text: &str, recipes: &'a [Recipe]) -> Option<Hit<'a>> {
    resolve_or_refuse(text, recipes).ok().flatten()
}

/// `resolve`, with the refusal kept: the hint for the refused amount, and the text after its piece.
pub fn resolve_or_refuse<'a>(text: &str, recipes: &'a [Recipe]) -> Result<Option<Hit<'a>>, Refused> {
    match exact(text, recipes) {
        Exact::Hit(hit) => Ok(Some(hit)),
        Exact::Refuse(r) => Err(r),
        Exact::Miss => fuzzy(text, recipes).transpose(),
    }
}

/// A serving count above this is not a serving count (`salmon 150` is a weight with no unit).
const MAX_SERVINGS: f64 = 12.0;

const NAME_STOP: [&str; 10] = ["the", "and", "with", "made", "of", "a", "an", "my", "some", "recipe"];

/// A recipe line the typed text could not use. `rest` is the text after the refused piece.
#[derive(Clone, Debug)]
pub struct Refused {
    pub unknown: nutrition::Unknown,
    pub rest: String,
}

enum Exact<'a> {
    Hit(Hit<'a>),
    Refuse(Refused),
    Miss,
}

/// What a recipe hit scales by: nothing typed, a count of servings, or a typed weight.
#[derive(Debug, PartialEq)]
enum Qty {
    One,
    Count(f64),
    Weight(f64),
}

/// The one rule for the amounts typed next to a recipe name (R1–R7). `amounts` are the amounts outside the name,
/// `name_words` every name word of the recipe, `ref_food` the food a weight is read against.
fn decide(amounts: &[nutrition::Amount], name_words: &[String], ref_food: &str) -> Result<Qty, String> {
    use crate::nutrition::Kind;
    // R1: a negative, unreadable or range amount is refused.
    for a in amounts {
        match a.kind {
            Kind::Negative => return Err("negative amount; say how much you had".to_string()),
            Kind::Unreadable => return Err("amount not readable; write it as 100, 1/2 or ½".to_string()),
            Kind::Range => return Err("a range; send one number".to_string()),
            _ => {}
        }
    }
    // R2: a volume is not a serving count.
    if let Some(a) = amounts.iter().find(|a| matches!(a.kind, Kind::Volume(_))) {
        return Err(format!("'{}' isn't a serving; give the {ref_food} weight in g, or a count", a.unit));
    }
    let masses: Vec<&nutrition::Amount> = amounts.iter().filter(|a| a.kind == Kind::Mass).collect();
    let counts: Vec<&nutrition::Amount> = amounts.iter().filter(|a| matches!(a.kind, Kind::Serving | Kind::Multiplier | Kind::Piece { .. } | Kind::Bare)).collect();
    // R3: one weight and one count at most.
    if masses.len() > 1 || counts.len() > 1 {
        return Err("two amounts in one line; send one".to_string());
    }
    let weight = |w: f64| -> Result<Qty, String> {
        if !w.is_finite() || w <= 0.0 {
            Err("zero amount; nothing logged".to_string())
        } else if w > 10_000.0 {
            Err("over 10 kg; check the amount".to_string())
        } else {
            Ok(Qty::Weight(w))
        }
    };
    match (masses.first(), counts.first()) {
        // R7: nothing typed.
        (None, None) => Ok(Qty::One),
        // R5: a weight alone.
        (Some(m), None) => weight(m.value),
        // R4: a weight and a count. A multiplier scales the weight; a bracketed weight is the weight.
        (Some(m), Some(c)) => {
            let w = if c.kind == Kind::Multiplier {
                m.value * c.value
            } else if m.bracketed {
                m.value
            } else {
                return Err("two amounts in one line; send one".to_string());
            };
            weight(w)
        }
        // R6: a count alone.
        (None, Some(c)) => {
            if c.kind == Kind::Bare && !c.after.is_empty() && !NAME_STOP.contains(&c.after.as_str()) && !name_words.iter().any(|n| word_match(&c.after, n)) {
                return Err(format!("unknown unit '{}'; give g or a count", c.after));
            }
            if c.value == 0.0 {
                return Err("zero servings; nothing logged".to_string());
            }
            if c.value > MAX_SERVINGS {
                let v = fmt(c.value);
                return Err(format!("{v} servings? for a weight add g ({v} g)"));
            }
            Ok(Qty::Count(c.value))
        }
    }
}

/// The hit fields a `Qty` gives: servings, explicit, weight_g.
fn qty_fields(q: &Qty) -> (f64, bool, Option<f64>) {
    match *q {
        Qty::One => (1.0, false, None),
        Qty::Count(v) => (v, true, None),
        Qty::Weight(w) => (1.0, false, Some(w)),
    }
}

/// Every name word of every name of the recipe.
fn all_name_words(r: &Recipe) -> Vec<String> {
    r.names.iter().flat_map(|n| name_words(n)).collect()
}

/// The food a recipe's weights are read against, for the replies.
fn ref_food_name(r: &Recipe) -> String {
    reference(r).map_or_else(|| "main ingredient".to_string(), |i| i.food.name.to_string())
}

/// The exact name path. Each piece is read on its own: the longest name wins; ties keep the first recipe, then the first piece.
fn exact<'a>(text: &str, recipes: &'a [Recipe]) -> Exact<'a> {
    // A recipe name with " and " in it is matched before the text is split at " and ".
    let keep: Vec<String> = recipes.iter().flat_map(|r| r.names.iter()).map(|n| n.to_ascii_lowercase()).filter(|n| n.contains(" and ")).collect();
    let pieces = nutrition::split_pieces_keeping(text, &keep);
    let scans: Vec<(nutrition::Scan, Vec<(usize, usize, String)>)> = pieces
        .iter()
        .map(|p| {
            let sc = nutrition::scan(p);
            let toks = tokenize(&sc.text);
            (sc, toks)
        })
        .collect();
    // (name words, piece, recipe, first token)
    let mut best: Option<(usize, usize, &'a Recipe, usize)> = None;
    for r in recipes {
        for name in &r.names {
            let words: Vec<String> = tokenize(name).into_iter().map(|t| t.2).collect();
            let k = words.len();
            if k == 0 {
                continue;
            }
            for (pi, (_, toks)) in scans.iter().enumerate() {
                if k > toks.len() {
                    continue;
                }
                for i in 0..=toks.len() - k {
                    if (0..k).all(|j| toks[i + j].2 == words[j]) && best.is_none_or(|(bk, _, _, _)| k > bk) {
                        best = Some((k, pi, r, i));
                    }
                }
            }
        }
    }
    let Some((k, pi, recipe, i)) = best else {
        return Exact::Miss;
    };
    let (sc, toks) = &scans[pi];
    let span = (toks[i].0, toks[i + k - 1].1);
    let matched = sc.text[span.0..span.1].to_string();
    // The name's own words are not read as amounts (`tofu 65 1/2 kg` keeps its 1/2 kg); anything else that overlaps the name is dropped.
    let amounts: Vec<nutrition::Amount> = nutrition::scan_outside(sc, span).into_iter().filter(|a| !(a.start < span.1 && span.0 < a.end)).collect();
    let other_pieces: Vec<String> = pieces.iter().enumerate().filter(|(j, _)| *j != pi).map(|(_, p)| p.clone()).collect();
    let nw = all_name_words(recipe);
    let ref_food = ref_food_name(recipe);
    match decide(&amounts, &nw, &ref_food) {
        Err(hint) => Exact::Refuse(Refused {
            unknown: nutrition::Unknown { text: pieces[pi].trim().to_string(), hint },
            rest: other_pieces.join("\n"),
        }),
        Ok(q) => {
            // The matched piece keeps what the name and its amounts did not use.
            let mut spans = vec![span];
            spans.extend(amounts.iter().map(|a| (a.start, a.end)));
            let leftover = nutrition::strip_spans(&sc.text, &spans);
            let mut parts: Vec<String> = Vec::new();
            for (j, p) in pieces.iter().enumerate() {
                if j != pi {
                    parts.push(p.clone());
                } else if !leftover.is_empty() {
                    parts.push(leftover.clone());
                }
            }
            let (servings, explicit, weight_g) = qty_fields(&q);
            Exact::Hit(Hit { recipe, servings, rest: parts.join("\n"), explicit, weight_g, fuzzy: false, matched })
        }
    }
}

/// Fuzzy pass: per piece and per recipe name, count distinct name words that match a piece word
/// (equal, a trailing `s` apart, or Levenshtein 1 for words of 4+ letters). Accept at least two
/// words and at least half of the name's words. The highest (hits, ingredient hits) wins.
fn fuzzy<'a>(text: &str, recipes: &'a [Recipe]) -> Option<Result<Hit<'a>, Refused>> {
    let pieces = nutrition::split_pieces(text);
    let mut best: Option<(usize, usize, usize, &'a Recipe)> = None;
    for r in recipes {
        let ing = ingredient_words(r);
        let anchors = anchor_words(r);
        for (pi, piece) in pieces.iter().enumerate() {
            let pw = words(&nutrition::scan(piece).text);
            // The main ingredient must be named: a different protein with the same side words is no hit.
            if !pw.iter().any(|w| anchors.iter().any(|a| word_match(w, a))) {
                continue;
            }
            let mut hits = 0;
            for name in &r.names {
                let nw = name_words(name);
                let h = nw.iter().filter(|n| pw.iter().any(|w| word_match(w, n))).count();
                if h >= 2 && 2 * h >= nw.len() && h > hits {
                    hits = h;
                }
            }
            if hits == 0 {
                continue;
            }
            // A piece word that is none of the name or ingredient words, and looks like food,
            // is food the recipe does not have: no fuzzy hit for this recipe.
            let extra = pw.iter().any(|w| {
                !NAME_STOP.contains(&w.as_str())
                    && !r.names.iter().any(|name| name_words(name).iter().any(|n| word_match(w, n)))
                    && !ing.iter().any(|g| word_match(w, g))
                    && nutrition::looks_like_food(w)
            });
            if extra {
                continue;
            }
            let mut seen: Vec<&String> = Vec::new();
            for w in &pw {
                if !seen.contains(&w) && ing.iter().any(|g| word_match(w, g)) {
                    seen.push(w);
                }
            }
            let ing_hits = seen.len();
            if best.is_none_or(|(h, g, _, _)| (hits, ing_hits) > (h, g)) {
                best = Some((hits, ing_hits, pi, r));
            }
        }
    }
    let (_, _, pi, recipe) = best?;
    let piece = &pieces[pi];
    let sc = nutrition::scan(piece);
    let nw = all_name_words(recipe);
    // A number word that is a word of the name ("One Pot Pasta") is part of the name, not an amount.
    let amounts: Vec<nutrition::Amount> = sc.amounts.iter().filter(|a| !(a.word && nw.contains(&sc.text[a.start..a.end].to_lowercase()))).cloned().collect();
    let rest = pieces.iter().enumerate().filter(|(j, _)| *j != pi).map(|(_, p)| p.clone()).collect::<Vec<_>>().join("\n");
    let ref_food = ref_food_name(recipe);
    Some(match decide(&amounts, &nw, &ref_food) {
        Err(hint) => Err(Refused { unknown: nutrition::Unknown { text: piece.trim().to_string(), hint }, rest }),
        Ok(q) => {
            let (servings, explicit, weight_g) = qty_fields(&q);
            Ok(Hit { recipe, servings, rest, explicit, weight_g, fuzzy: true, matched: piece.clone() })
        }
    })
}

/// Scale the recipe to what was typed. Explicit count wins; then a typed weight of the reference ingredient
/// (`honey garlic salmon 150g`); then a typed row for that food; else one serving.
pub fn scale(hit: &Hit, typed: &nutrition::Parsed) -> (f64, Scaled) {
    let Some(heaviest) = reference(hit.recipe) else {
        return (1.0, Scaled::Assumed);
    };
    let food: &'static str = heaviest.food.name;
    let (ratio, how) = if hit.explicit {
        (hit.servings, Scaled::Count)
    } else if let Some(w) = hit.weight_g {
        (w / heaviest.grams, Scaled::Weight { food, grams: w })
    } else {
        let rows: Vec<f64> = typed.items.iter().filter(|i| i.food.name == food).map(|i| i.grams).collect();
        if rows.is_empty() {
            return (1.0, Scaled::Assumed);
        }
        let g: f64 = rows.iter().sum();
        (g / heaviest.grams, Scaled::Typed { food, grams: g })
    };
    if ratio.is_finite() && ratio > 0.0 {
        (ratio, how)
    } else {
        (1.0, Scaled::Assumed)
    }
}

pub fn ingredient_pieces(hit: &Hit) -> Vec<String> {
    hit.recipe
        .lines
        .iter()
        .map(|line| match nutrition::quantity(line) {
            Some((v, rest)) => format!("{} {}", fmt(v * hit.servings), rest).trim_end().to_string(),
            None => line.clone(),
        })
        .collect()
}

fn fmt(v: f64) -> String {
    format!("{}", (v * 100.0).round() / 100.0)
}

fn split_frontmatter(text: &str) -> (&str, &str) {
    let mut it = text.split_inclusive('\n');
    match it.next() {
        Some(first) if first.trim_end() == "---" => {
            let mut off = first.len();
            for line in it {
                if line.trim_end() == "---" {
                    return (&text[first.len()..off], &text[off + line.len()..]);
                }
                off += line.len();
            }
            ("", text)
        }
        _ => ("", text),
    }
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string()
}

fn frontmatter_aliases(fm: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = fm.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = line.strip_prefix("aliases:") else { continue };
        let rest = rest.trim();
        if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            out.extend(inner.split(',').map(unquote).filter(|s| !s.is_empty()));
        } else {
            for item in lines.by_ref() {
                match item.trim_start().strip_prefix("- ") {
                    Some(v) => out.push(unquote(v)),
                    None => break,
                }
            }
        }
        break;
    }
    out
}

/// A table header cell that marks an amount column (or an ingredient table).
fn qty_header(cell: &str) -> bool {
    let c = cell.trim().to_lowercase();
    c == "recipe" || QTY_HEADERS.contains(&c.as_str()) || c.contains("ingredient")
}

const QTY_HEADERS: [&str; 6] = ["amount", "qty", "quantity", "g", "grams", "weight"];

/// An amount from a table cell: `100`, `1 1/2`, or a range with a unit, `1–2 cups` (the midpoint, in
/// that unit). A unitless range, `3-6`, keeps its first number as grams. The range (low, high, and
/// whether it had a unit) is returned so the caller can report it as assumed.
fn amount(cell: &str) -> Option<(f64, String, Option<(f64, f64, bool)>)> {
    let cell = cell.trim();
    if let Some(pos) = cell.find(['-', '–']).filter(|&p| p > 0) {
        let sep = cell[pos..].chars().next().map_or(1, char::len_utf8);
        if let (Some((a, _)), Some((b, tail))) =
            (nutrition::quantity(cell[..pos].trim()), nutrition::quantity(cell[pos + sep..].trim_start()))
        {
            return Some(match tail.split_whitespace().next() {
                Some(unit) => ((a + b) / 2.0, unit.to_string(), Some((a, b, true))),
                None => (a, "g".to_string(), Some((a, b, false))),
            });
        }
    }
    let (v, rest) = nutrition::quantity(cell)?;
    Some((v, rest.split_whitespace().next().unwrap_or("g").to_string(), None))
}

/// Ingredient lines in note order: table rows (recipe column, else column 1) and
/// bullets under an "ingredient" heading. Returns (lines, skipped, assumed).
fn ingredient_lines(body: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut lines = Vec::new();
    let mut skipped = Vec::new();
    let mut assumed = Vec::new();
    let mut in_ingredients = false;
    let mut in_table = false;
    let mut table_ok = false;
    let mut qty_col = 1;
    for raw in body.lines() {
        let line = raw.trim();
        if line.starts_with('|') {
            let cells = cells_of(line);
            if !in_table {
                in_table = true;
                table_ok = in_ingredients || cells.iter().any(|c| qty_header(c));
                qty_col = cells
                    .iter()
                    .position(|c| c.eq_ignore_ascii_case("recipe"))
                    .or_else(|| cells.iter().position(|c| QTY_HEADERS.contains(&c.trim().to_lowercase().as_str())))
                    .unwrap_or(1);
                continue;
            }
            if !table_ok || is_separator(&cells) {
                continue;
            }
            let name = cells
                .first()
                .map(|c| c.split([',', '(']).next().unwrap_or("").trim().trim_matches('*').trim().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let cell = cells.get(qty_col).map(|c| c.replace('*', "")).unwrap_or_default();
            match amount(&cell) {
                Some((v, unit, range)) => {
                    match range {
                        Some((_, _, true)) => {
                            assumed.push(format!("{name}: {} read as midpoint {} {unit}", cell.trim(), fmt(v)));
                        }
                        Some((a, b, false)) => {
                            assumed.push(format!("{name}: {}-{} read as {} g", fmt(a), fmt(b), fmt(a)));
                        }
                        None => {}
                    }
                    lines.push(format!("{} {} {}", fmt(v), unit, name));
                }
                None => skipped.push(format!("{name}: no amount")),
            }
            continue;
        }
        in_table = false;
        if line.starts_with('#') {
            in_ingredients = line.trim_start_matches('#').to_lowercase().contains("ingredient");
        } else if in_ingredients && (line.starts_with("- ") || line.starts_with("* ")) {
            let text = line[2..].split(',').next().unwrap_or("").trim();
            if !text.is_empty() {
                lines.push(text.to_string());
            }
        }
    }
    (lines, skipped, assumed)
}

fn cells_of(line: &str) -> Vec<String> {
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '/' | '×')
}

/// Word tokens as (start, end, lowercase text); a '.' counts only inside a number.
fn tokenize(text: &str) -> Vec<(usize, usize, String)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (s, c) = chars[i];
        if !is_word(c) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() {
            let cj = chars[j].1;
            let dot = cj == '.' && j > i && chars.get(j + 1).is_some_and(|x| x.1.is_ascii_digit());
            if is_word(cj) || dot {
                j += 1;
            } else {
                break;
            }
        }
        let end = chars.get(j).map_or(text.len(), |x| x.0);
        out.push((s, end, text[s..end].to_lowercase()));
        i = j;
    }
    out
}

/// Lowercase words of 3+ letters, the only words the fuzzy pass compares.
fn words(text: &str) -> Vec<String> {
    tokenize(text).into_iter().map(|t| t.2).filter(|w| w.chars().count() >= 3 && w.chars().all(char::is_alphabetic)).collect()
}

fn name_words(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words(name) {
        if !NAME_STOP.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// Words of the recipe's ingredient lines (quantities and units dropped).
fn ingredient_words(r: &Recipe) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in &r.lines {
        let rest = nutrition::quantity(line).map_or(line.as_str(), |(_, rest)| rest);
        for w in words(rest) {
            if !out.contains(&w) {
                out.push(w);
            }
        }
    }
    out
}

/// The name words that name the recipe's main ingredient: the words of its reference food's name or aliases
/// that a name word matches. Empty when the title names no ingredient (then the recipe gets no fuzzy hit).
fn anchor_words(r: &Recipe) -> Vec<String> {
    let Some(item) = reference(r) else {
        return Vec::new();
    };
    let food = food_words(item.food);
    let mut out: Vec<String> = Vec::new();
    for w in all_name_words(r) {
        if food.iter().any(|f| word_match(&w, f)) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// The food's name and aliases as lowercase words.
fn food_words(f: &nutrition::Food) -> Vec<String> {
    std::iter::once(f.name).chain(f.aliases.iter().copied()).flat_map(words).collect()
}

pub(crate) fn word_match(a: &str, b: &str) -> bool {
    a == b
        || a.strip_suffix('s') == Some(b)
        || b.strip_suffix('s') == Some(a)
        || (a.chars().count() >= 4 && b.chars().count() >= 4 && levenshtein(a, b) <= 1)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The ingredient a recipe scales by: among the items whose food a title word names (a food word word_matches a
/// name word), the one with the most protein, ties to more grams. If the title names none, the heaviest item.
pub fn reference(r: &Recipe) -> Option<nutrition::Item> {
    let items = nutrition::parse_meal(&r.lines.join("\n")).items;
    let names = all_name_words(r);
    let named: Vec<nutrition::Item> = items
        .iter()
        .filter(|it| food_words(it.food).iter().any(|f| names.iter().any(|n| word_match(n, f))))
        .cloned()
        .collect();
    if named.is_empty() {
        return items.into_iter().max_by(|a, b| a.grams.total_cmp(&b.grams));
    }
    let protein = |it: &nutrition::Item| it.grams * it.food.protein / 100.0;
    named.into_iter().max_by(|a, b| protein(a).total_cmp(&protein(b)).then(a.grams.total_cmp(&b.grams)))
}
