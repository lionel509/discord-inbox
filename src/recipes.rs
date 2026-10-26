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
}

#[derive(Clone, Debug)]
pub struct Hit<'a> {
    pub recipe: &'a Recipe,
    pub servings: f64,
    pub rest: String,
}

/// Every `*.md` directly in `dir` that yields at least one ingredient line, sorted by name.
pub fn load(dir: &Path) -> Vec<Recipe> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Recipe> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
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
    let (lines, skipped) = ingredient_lines(body);
    if lines.is_empty() {
        return None;
    }
    Some(Recipe { name: stem.to_string(), names, lines, skipped })
}

pub fn resolve<'a>(text: &str, recipes: &'a [Recipe]) -> Option<Hit<'a>> {
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
    let (i, k, recipe) = best?;
    let w = |j: usize| toks[j].2.as_str();
    let span = |j: usize| (toks[j].0, toks[j].1);
    let mut removed = vec![(toks[i].0, toks[i + k - 1].1)];
    let mut servings = None;
    if i >= 1 && w(i - 1) == "half" {
        servings = Some(0.5);
        removed.push(span(i - 1));
    }
    if servings.is_none() && i >= 2 && matches!(w(i - 1), "serving" | "servings" | "portion" | "portions") {
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
    let after = i + k;
    if servings.is_none() && after < toks.len() {
        if let Some(n) = count(w(after)) {
            servings = Some(n);
            removed.push(span(after));
        } else if matches!(w(after), "x" | "×") && after + 1 < toks.len() {
            if let Some(n) = count(w(after + 1)) {
                servings = Some(n);
                removed.extend([span(after), span(after + 1)]);
            }
        }
    }
    let mut rest = String::new();
    for (idx, ch) in text.char_indices() {
        if !removed.iter().any(|&(s, e)| idx >= s && idx < e) {
            rest.push(ch);
        }
    }
    Some(Hit { recipe, servings: servings.unwrap_or(1.0), rest: rest.trim().to_string() })
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

/// Ingredient lines in note order: table rows (recipe column, else column 1) and
/// bullets under an "ingredient" heading. Returns (lines, skipped).
fn ingredient_lines(body: &str) -> (Vec<String>, Vec<String>) {
    let mut lines = Vec::new();
    let mut skipped = Vec::new();
    let mut in_ingredients = false;
    let mut qty_col = 1;
    let mut prev_table = false;
    for raw in body.lines() {
        let line = raw.trim();
        if line.starts_with('|') {
            let cells = cells_of(line);
            if !prev_table {
                qty_col = cells.iter().position(|c| c.eq_ignore_ascii_case("recipe")).unwrap_or(1);
                prev_table = true;
                continue;
            }
            prev_table = true;
            if is_separator(&cells) {
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
            let first = cell.trim().split(['-', '–']).next().unwrap_or("").trim().to_string();
            match nutrition::quantity(&first) {
                Some((v, rest)) => {
                    let unit = rest.split_whitespace().next().unwrap_or("g");
                    lines.push(format!("{} {} {}", fmt(v), unit, name));
                }
                None => skipped.push(format!("{name}: no amount")),
            }
            continue;
        }
        prev_table = false;
        if line.starts_with('#') {
            in_ingredients = line.trim_start_matches('#').to_lowercase().contains("ingredient");
        } else if in_ingredients && (line.starts_with("- ") || line.starts_with("* ")) {
            let text = line[2..].split(',').next().unwrap_or("").trim();
            if !text.is_empty() {
                lines.push(text.to_string());
            }
        }
    }
    (lines, skipped)
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
