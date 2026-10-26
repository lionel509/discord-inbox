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
    match exact(text, recipes) {
        Exact::Hit(hit) => Some(hit),
        Exact::Zero => None,
        Exact::Miss => fuzzy(text, recipes),
    }
}

enum Exact<'a> {
    Hit(Hit<'a>),
    /// A count of zero next to the name: no hit, and no fuzzy retry either.
    Zero,
    Miss,
}

fn exact<'a>(text: &str, recipes: &'a [Recipe]) -> Exact<'a> {
    let toks = tokenize(text);
    // Longest matching phrase wins; ties keep the first recipe.
    let mut best: Option<(usize, usize, &'a Recipe)> = None;
    for r in recipes {
        for name in &r.names {
            let words: Vec<String> = tokenize(name).into_iter().map(|t| t.2).collect();
            let k = words.len();
            if k == 0 || k > toks.len() {
                continue;
            }
            for i in 0..=toks.len() - k {
                if (0..k).all(|j| toks[i + j].2 == words[j]) && best.is_none_or(|(_, bk, _)| k > bk) {
                    best = Some((i, k, r));
                }
            }
        }
    }
    let Some((i, k, recipe)) = best else { return Exact::Miss };
    let w = |j: usize| toks[j].2.as_str();
    let span = |j: usize| (toks[j].0, toks[j].1);
    let after = i + k;
    let counts_word = |j: usize| matches!(w(j), "x" | "×" | "serving" | "servings" | "portion" | "portions");
    // A count of zero is not a serving count: no hit at all.
    let zero = (i >= 1 && is_zero(w(i - 1)))
        || (i >= 2 && counts_word(i - 1) && is_zero(w(i - 2)))
        || (i >= 3 && w(i - 1) == "of" && counts_word(i - 2) && is_zero(w(i - 3)))
        || (after < toks.len() && is_zero(w(after)))
        || (after + 1 < toks.len() && matches!(w(after), "x" | "×") && is_zero(w(after + 1)))
        || (i >= 2 && matches!(w(i - 1), "x" | "×") && is_zero(w(i - 2)));
    if zero {
        return Exact::Zero;
    }
    let matched = text[toks[i].0..toks[i + k - 1].1].to_string();
    let mut removed = vec![(toks[i].0, toks[i + k - 1].1)];
    let mut servings = None;
    if i >= 1 && w(i - 1) == "half" {
        servings = Some(0.5);
        removed.push(span(i - 1));
    }
    if servings.is_none() && i >= 3 && w(i - 1) == "of" && matches!(w(i - 2), "serving" | "servings" | "portion" | "portions") {
        // "2 servings of <name>"
        if let Some(n) = count(w(i - 3)) {
            servings = Some(n);
            removed.extend([span(i - 3), span(i - 2), span(i - 1)]);
        }
    }
    if servings.is_none() && i >= 2 && matches!(w(i - 1), "serving" | "servings" | "portion" | "portions") {
        if let Some(n) = count(w(i - 2)) {
            if i >= 3 && is_whole(w(i - 3)) && is_fraction(w(i - 2)) {
                // "1 1/2 servings": the whole number and the fraction add up.
                servings = Some(w(i - 3).parse::<f64>().unwrap_or(0.0) + n);
                removed.extend([span(i - 3), span(i - 2), span(i - 1)]);
            } else {
                servings = Some(n);
                removed.extend([span(i - 2), span(i - 1)]);
            }
        }
    }
    if servings.is_none() && i >= 2 && matches!(w(i - 1), "x" | "×") {
        if let Some(n) = count(w(i - 2)) {
            servings = Some(n);
            removed.extend([span(i - 2), span(i - 1)]);
        }
    }
    if servings.is_none() && i >= 1 {
        if let Some(n) = count(w(i - 1)) {
            servings = Some(n);
            removed.push(span(i - 1));
        }
    }
    if servings.is_none() && after < toks.len() {
        // The index of the first token after the count, if a count was read after the name.
        let mut next = None;
        if is_whole(w(after)) && after + 1 < toks.len() && is_fraction(w(after + 1)) {
            // "1 1/2 servings": the mixed number reads as 1.5.
            if let Some((v, _)) = nutrition::quantity(&text[toks[after].0..]) {
                servings = Some(v);
                removed.extend([span(after), span(after + 1)]);
                next = Some(after + 2);
            }
        } else if let Some(n) = count(w(after)) {
            servings = Some(n);
            removed.push(span(after));
            next = Some(after + 1);
        } else if matches!(w(after), "x" | "×") && after + 1 < toks.len() {
            if let Some(n) = count(w(after + 1)) {
                servings = Some(n);
                removed.extend([span(after), span(after + 1)]);
                next = Some(after + 2);
            }
        }
        // The serving word that follows the count goes with it.
        if let Some(j) = next.filter(|&j| j < toks.len()) {
            if matches!(w(j), "serving" | "servings" | "portion" | "portions") {
                removed.push(span(j));
            }
        }
    }
    // A typed weight in the piece that holds the name scales the recipe, as on the fuzzy path,
    // and comes out of the rest so it is never read as an unknown.
    let mut weight_g = None;
    if let Some(piece) = nutrition::split_pieces(text).iter().find(|p| p.contains(matched.as_str())) {
        if let Some(ps) = text.find(piece.as_str()) {
            let ptoks = tokenize(piece);
            if let Some((g, a, b)) = weight_span(&ptoks) {
                weight_g = Some(g);
                removed.push((ps + ptoks[a].0, ps + ptoks[b].1));
            }
        }
    }
    let mut rest = String::new();
    for (idx, ch) in text.char_indices() {
        if !removed.iter().any(|&(s, e)| idx >= s && idx < e) {
            rest.push(ch);
        }
    }
    Exact::Hit(Hit {
        recipe,
        servings: servings.unwrap_or(1.0),
        rest: rest.trim().to_string(),
        explicit: servings.is_some(),
        weight_g,
        fuzzy: false,
        matched,
    })
}

const NAME_STOP: [&str; 10] = ["the", "and", "with", "made", "of", "a", "an", "my", "some", "recipe"];

/// Fuzzy pass: per piece and per recipe name, count distinct name words that match a piece word
/// (equal, a trailing `s` apart, or Levenshtein 1 for words of 4+ letters). Accept at least two
/// words and at least half of the name's words. The highest (hits, ingredient hits) wins.
fn fuzzy<'a>(text: &str, recipes: &'a [Recipe]) -> Option<Hit<'a>> {
    let pieces = nutrition::split_pieces(text);
    let mut best: Option<(usize, usize, usize, &'a Recipe)> = None;
    for r in recipes {
        let ing = ingredient_words(r);
        for (pi, piece) in pieces.iter().enumerate() {
            let pw = words(piece);
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
    let piece = pieces[pi].clone();
    let toks = tokenize(&piece);
    if toks.iter().any(|t| is_zero(&t.2)) {
        return None;
    }
    let n = toks.len();
    let w = |j: usize| toks[j].2.as_str();
    let mut found: Option<f64> = None;
    for j in 0..n {
        // "150 g" is a weight, not a count.
        if toks.get(j + 1).is_some_and(|t| unit_factor(&t.2).is_some()) && w(j).parse::<f64>().is_ok() {
            continue;
        }
        if w(j) == "half" {
            found = Some(0.5);
            break;
        }
        if j + 1 < n && is_whole(w(j)) && is_fraction(w(j + 1)) {
            found = Some(w(j).parse::<f64>().unwrap_or(0.0) + count(w(j + 1)).unwrap_or(0.0));
            break;
        }
        if let Some(v) = count(w(j)) {
            found = Some(v);
            break;
        }
    }
    let rest = match text.find(&piece) {
        Some(s) => {
            let mut r = text.to_string();
            r.replace_range(s..s + piece.len(), "");
            r
        }
        None => text.to_string(),
    };
    let rest = rest.trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '+')).to_string();
    Some(Hit {
        recipe,
        servings: found.unwrap_or(1.0),
        rest,
        explicit: found.is_some(),
        weight_g: weight_of(&toks),
        fuzzy: true,
        matched: piece,
    })
}

/// Scale the recipe to what was typed. Explicit count wins; then a typed weight of the heaviest
/// ingredient (`honey garlic salmon 150g`); then a typed row for that food; else one serving.
pub fn scale(hit: &Hit, typed: &nutrition::Parsed) -> (f64, Scaled) {
    let base = nutrition::parse_meal(&hit.recipe.lines.join("\n"));
    let Some(heaviest) = base.items.iter().max_by(|a, b| a.grams.total_cmp(&b.grams)) else {
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

fn word_match(a: &str, b: &str) -> bool {
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

/// A typed weight in the piece: `150g`, `1.5kg`, `6 oz`, or a number then a unit word. Grams, and the
/// first and last token it covers.
fn weight_span(toks: &[(usize, usize, String)]) -> Option<(f64, usize, usize)> {
    for (j, (_, _, w)) in toks.iter().enumerate() {
        let split = w.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(w.len());
        let Ok(n) = w[..split].parse::<f64>() else { continue };
        let (unit, last) = if split == w.len() {
            (toks.get(j + 1).map_or("", |t| t.2.as_str()), j + 1)
        } else {
            (&w[split..], j)
        };
        if let Some(f) = unit_factor(unit) {
            let g = n * f;
            if g.is_finite() && g > 0.0 {
                return Some((g, j, last));
            }
        }
    }
    None
}

fn weight_of(toks: &[(usize, usize, String)]) -> Option<f64> {
    weight_span(toks).map(|(g, _, _)| g)
}

fn unit_factor(u: &str) -> Option<f64> {
    match u {
        "g" | "gram" | "grams" => Some(1.0),
        "kg" | "kgs" | "kilo" | "kilos" | "kilogram" | "kilograms" => Some(1000.0),
        "oz" | "ounce" | "ounces" => Some(28.35),
        _ => None,
    }
}

fn is_whole(s: &str) -> bool {
    s.parse::<u32>().is_ok()
}

fn is_fraction(s: &str) -> bool {
    (s.contains('/') || matches!(s, "½" | "¼" | "¾")) && count(s).is_some()
}

fn is_zero(s: &str) -> bool {
    s.trim_start_matches(['x', '×']).trim_end_matches(['x', '×']).parse::<f64>() == Ok(0.0)
}

/// A serving count: "2", "0.5", "1/2", "x2", "2x", "×2".
fn count(s: &str) -> Option<f64> {
    let s = s.trim_start_matches(['x', '×']).trim_end_matches(['x', '×']);
    let v = match s {
        "½" => 0.5,
        "¼" => 0.25,
        "¾" => 0.75,
        _ => match s.split_once('/') {
            Some((a, b)) => a.parse::<f64>().ok()? / b.parse::<f64>().ok()?,
            None => s.parse::<f64>().ok()?,
        },
    };
    (v.is_finite() && v > 0.0).then_some(v)
}
