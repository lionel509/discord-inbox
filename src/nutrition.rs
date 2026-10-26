//! Local nutrition table and line parser (U1). No model or network calls; every number comes from `FOODS`.
use std::fmt;

/// One row of the table. Per-100 g values. `carbs` is available carbohydrate (USDA carbs minus fibre) for the
/// vegetables where that is what keeps the Atwater check honest; everything else is the USDA figure.
#[derive(Debug)]
pub struct Food {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub kcal: f64,
    pub protein: f64,
    pub carbs: f64,
    pub fat: f64,
    pub veg: bool,
    pub cup_g: Option<f64>,
    pub piece_g: Option<f64>,
    pub portion_g: f64,
    pub source: &'static str,
}

pub const FOODS: &[Food] = &[
    Food { name: "salmon", aliases: &["salmon fillet", "atlantic salmon", "skinless salmon"], kcal: 208.0, protein: 20.4, carbs: 0.0, fat: 13.4, veg: false, cup_g: None, piece_g: Some(150.0), portion_g: 100.0, source: "USDA SR 15236" },
    Food { name: "dry rice", aliases: &["raw rice", "uncooked rice", "white rice dry", "rice dry"], kcal: 365.0, protein: 7.1, carbs: 80.0, fat: 0.7, veg: false, cup_g: Some(185.0), piece_g: None, portion_g: 92.5, source: "USDA SR 20044" },
    Food { name: "cooked rice", aliases: &["rice cooked", "white rice cooked", "steamed rice"], kcal: 130.0, protein: 2.7, carbs: 28.2, fat: 0.3, veg: false, cup_g: Some(158.0), piece_g: None, portion_g: 158.0, source: "USDA SR 20045" },
    Food { name: "cabbage", aliases: &["glazed cabbage", "napa cabbage", "green cabbage"], kcal: 25.0, protein: 1.3, carbs: 5.8, fat: 0.1, veg: true, cup_g: Some(89.0), piece_g: Some(900.0), portion_g: 100.0, source: "USDA SR 11109" },
    Food { name: "oil", aliases: &["vegetable oil", "neutral oil", "canola oil", "cooking oil"], kcal: 884.0, protein: 0.0, carbs: 0.0, fat: 100.0, veg: false, cup_g: Some(217.6), piece_g: None, portion_g: 13.6, source: "USDA SR 04582" },
    Food { name: "olive oil", aliases: &["extra virgin olive oil"], kcal: 884.0, protein: 0.0, carbs: 0.0, fat: 100.0, veg: false, cup_g: Some(216.0), piece_g: None, portion_g: 13.5, source: "USDA SR (recall)" },
    Food { name: "sesame oil", aliases: &["toasted sesame oil"], kcal: 884.0, protein: 0.0, carbs: 0.0, fat: 100.0, veg: false, cup_g: Some(218.0), piece_g: None, portion_g: 13.6, source: "USDA SR (recall)" },
    Food { name: "butter", aliases: &["unsalted butter"], kcal: 717.0, protein: 0.9, carbs: 0.1, fat: 81.1, veg: false, cup_g: Some(227.0), piece_g: None, portion_g: 14.0, source: "USDA SR (recall)" },
    Food { name: "egg", aliases: &["whole egg"], kcal: 143.0, protein: 12.6, carbs: 0.7, fat: 9.5, veg: false, cup_g: None, piece_g: Some(50.0), portion_g: 50.0, source: "USDA SR (recall)" },
    Food { name: "chicken breast", aliases: &["chicken breast raw", "boneless chicken breast"], kcal: 120.0, protein: 22.5, carbs: 0.0, fat: 2.6, veg: false, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "chicken thigh", aliases: &["boneless chicken thigh"], kcal: 119.0, protein: 19.6, carbs: 0.0, fat: 3.9, veg: false, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "ground beef", aliases: &["beef mince", "minced beef"], kcal: 215.0, protein: 17.2, carbs: 0.0, fat: 15.4, veg: false, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "ground pork", aliases: &["pork mince", "minced pork"], kcal: 263.0, protein: 16.9, carbs: 0.0, fat: 21.2, veg: false, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "pork belly", aliases: &[], kcal: 518.0, protein: 9.3, carbs: 0.0, fat: 53.0, veg: false, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "cod", aliases: &["atlantic cod"], kcal: 82.0, protein: 17.8, carbs: 0.0, fat: 0.7, veg: false, cup_g: None, piece_g: Some(150.0), portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "shrimp", aliases: &["raw shrimp", "prawn"], kcal: 85.0, protein: 20.1, carbs: 0.0, fat: 0.5, veg: false, cup_g: None, piece_g: Some(6.0), portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "tuna", aliases: &["canned tuna", "tuna in water"], kcal: 116.0, protein: 25.5, carbs: 0.0, fat: 0.8, veg: false, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall), drained" },
    Food { name: "tofu", aliases: &["firm tofu"], kcal: 144.0, protein: 17.3, carbs: 2.8, fat: 8.7, veg: false, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "dry pasta", aliases: &["uncooked pasta", "raw pasta", "spaghetti dry"], kcal: 371.0, protein: 13.0, carbs: 74.7, fat: 1.5, veg: false, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "cooked pasta", aliases: &["pasta cooked", "spaghetti cooked"], kcal: 158.0, protein: 5.8, carbs: 30.9, fat: 0.9, veg: false, cup_g: Some(140.0), piece_g: None, portion_g: 140.0, source: "USDA SR (recall)" },
    Food { name: "instant noodles", aliases: &["ramen", "instant ramen", "ramen noodles"], kcal: 450.0, protein: 9.0, carbs: 62.0, fat: 19.0, veg: false, cup_g: None, piece_g: Some(85.0), portion_g: 85.0, source: "label (recall, check the pack)" },
    Food { name: "bread", aliases: &["white bread", "toast"], kcal: 265.0, protein: 9.0, carbs: 49.0, fat: 3.2, veg: false, cup_g: None, piece_g: Some(30.0), portion_g: 30.0, source: "USDA SR (recall)" },
    Food { name: "oats", aliases: &["rolled oats", "oatmeal dry"], kcal: 389.0, protein: 16.9, carbs: 66.3, fat: 6.9, veg: false, cup_g: Some(81.0), piece_g: None, portion_g: 40.0, source: "USDA SR (recall)" },
    Food { name: "milk", aliases: &["whole milk"], kcal: 61.0, protein: 3.2, carbs: 4.8, fat: 3.3, veg: false, cup_g: Some(244.0), piece_g: None, portion_g: 244.0, source: "USDA SR (recall)" },
    Food { name: "greek yogurt", aliases: &["plain greek yogurt"], kcal: 97.0, protein: 9.0, carbs: 3.9, fat: 5.0, veg: false, cup_g: Some(245.0), piece_g: None, portion_g: 170.0, source: "USDA SR (recall)" },
    Food { name: "cheddar", aliases: &["cheddar cheese"], kcal: 403.0, protein: 24.9, carbs: 1.3, fat: 33.1, veg: false, cup_g: Some(113.0), piece_g: None, portion_g: 30.0, source: "USDA SR (recall)" },
    Food { name: "peanut butter", aliases: &["peanut butter smooth"], kcal: 588.0, protein: 25.1, carbs: 20.0, fat: 50.4, veg: false, cup_g: Some(258.0), piece_g: None, portion_g: 32.0, source: "USDA SR (recall)" },
    Food { name: "banana", aliases: &[], kcal: 89.0, protein: 1.1, carbs: 22.8, fat: 0.3, veg: false, cup_g: None, piece_g: Some(118.0), portion_g: 118.0, source: "USDA SR (recall)" },
    Food { name: "apple", aliases: &[], kcal: 52.0, protein: 0.3, carbs: 13.8, fat: 0.2, veg: false, cup_g: Some(125.0), piece_g: Some(182.0), portion_g: 182.0, source: "USDA SR (recall)" },
    Food { name: "avocado", aliases: &[], kcal: 160.0, protein: 2.0, carbs: 8.5, fat: 14.7, veg: false, cup_g: Some(150.0), piece_g: Some(150.0), portion_g: 150.0, source: "USDA SR (recall)" },
    Food { name: "potato", aliases: &["russet potato", "white potato"], kcal: 77.0, protein: 2.0, carbs: 17.5, fat: 0.1, veg: false, cup_g: Some(150.0), piece_g: Some(173.0), portion_g: 150.0, source: "USDA SR (recall)" },
    Food { name: "sweet potato", aliases: &[], kcal: 86.0, protein: 1.6, carbs: 20.1, fat: 0.1, veg: false, cup_g: Some(133.0), piece_g: Some(130.0), portion_g: 130.0, source: "USDA SR (recall)" },
    Food { name: "soy sauce", aliases: &["shoyu"], kcal: 53.0, protein: 8.1, carbs: 4.9, fat: 0.6, veg: false, cup_g: Some(255.0), piece_g: None, portion_g: 15.0, source: "USDA SR (recall)" },
    Food { name: "honey", aliases: &[], kcal: 304.0, protein: 0.3, carbs: 82.4, fat: 0.0, veg: false, cup_g: Some(339.0), piece_g: None, portion_g: 21.0, source: "USDA SR (recall)" },
    Food { name: "sugar", aliases: &["white sugar", "granulated sugar"], kcal: 387.0, protein: 0.0, carbs: 100.0, fat: 0.0, veg: false, cup_g: Some(200.0), piece_g: None, portion_g: 4.0, source: "USDA SR (recall)" },
    Food { name: "mayonnaise", aliases: &["mayo"], kcal: 680.0, protein: 1.0, carbs: 0.6, fat: 75.0, veg: false, cup_g: Some(220.0), piece_g: None, portion_g: 14.0, source: "USDA SR (recall)" },
    Food { name: "gochujang", aliases: &[], kcal: 200.0, protein: 4.5, carbs: 40.0, fat: 1.0, veg: false, cup_g: None, piece_g: None, portion_g: 15.0, source: "label (recall, check the tub)" },
    Food { name: "chili crisp", aliases: &["chilli crisp"], kcal: 500.0, protein: 2.0, carbs: 10.0, fat: 50.0, veg: false, cup_g: None, piece_g: None, portion_g: 10.0, source: "label (recall, check the jar)" },
    Food { name: "rice vinegar", aliases: &[], kcal: 18.0, protein: 0.0, carbs: 0.04, fat: 0.0, veg: false, cup_g: None, piece_g: None, portion_g: 15.0, source: "USDA SR (recall)" },
    Food { name: "black vinegar", aliases: &["chinkiang vinegar"], kcal: 20.0, protein: 0.0, carbs: 0.5, fat: 0.0, veg: false, cup_g: None, piece_g: None, portion_g: 15.0, source: "label (recall)" },
    Food { name: "garlic", aliases: &["garlic clove"], kcal: 149.0, protein: 6.4, carbs: 33.1, fat: 0.5, veg: false, cup_g: None, piece_g: Some(3.0), portion_g: 3.0, source: "USDA SR (recall)" },
    Food { name: "scallion", aliases: &["green onion", "spring onion"], kcal: 32.0, protein: 1.8, carbs: 5.0, fat: 0.2, veg: true, cup_g: Some(100.0), piece_g: None, portion_g: 30.0, source: "USDA SR (recall), carbs net of fibre" },
    Food { name: "cucumber", aliases: &[], kcal: 15.0, protein: 0.7, carbs: 3.6, fat: 0.1, veg: true, cup_g: Some(104.0), piece_g: Some(300.0), portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "broccoli", aliases: &[], kcal: 34.0, protein: 2.8, carbs: 4.0, fat: 0.4, veg: true, cup_g: Some(91.0), piece_g: None, portion_g: 100.0, source: "USDA SR (recall), carbs net of fibre" },
    Food { name: "spinach", aliases: &[], kcal: 23.0, protein: 2.9, carbs: 3.6, fat: 0.4, veg: true, cup_g: Some(30.0), piece_g: None, portion_g: 30.0, source: "USDA SR (recall)" },
    Food { name: "carrot", aliases: &[], kcal: 41.0, protein: 0.9, carbs: 9.6, fat: 0.2, veg: true, cup_g: Some(128.0), piece_g: Some(61.0), portion_g: 61.0, source: "USDA SR (recall)" },
    Food { name: "onion", aliases: &[], kcal: 40.0, protein: 1.1, carbs: 9.3, fat: 0.1, veg: true, cup_g: Some(160.0), piece_g: Some(110.0), portion_g: 110.0, source: "USDA SR (recall)" },
    Food { name: "bok choy", aliases: &["pak choi"], kcal: 13.0, protein: 1.5, carbs: 2.2, fat: 0.2, veg: true, cup_g: Some(70.0), piece_g: None, portion_g: 70.0, source: "USDA SR (recall)" },
    Food { name: "frozen mixed vegetables", aliases: &["frozen veg", "mixed vegetables"], kcal: 65.0, protein: 3.5, carbs: 12.2, fat: 0.3, veg: true, cup_g: Some(145.0), piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "peas", aliases: &["green peas", "frozen peas"], kcal: 81.0, protein: 5.4, carbs: 14.5, fat: 0.4, veg: true, cup_g: Some(145.0), piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "tomato", aliases: &[], kcal: 18.0, protein: 0.9, carbs: 3.9, fat: 0.2, veg: true, cup_g: Some(180.0), piece_g: Some(123.0), portion_g: 123.0, source: "USDA SR (recall)" },
    Food { name: "bell pepper", aliases: &["red pepper", "green pepper"], kcal: 31.0, protein: 1.0, carbs: 6.0, fat: 0.3, veg: true, cup_g: Some(149.0), piece_g: Some(119.0), portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "kimchi", aliases: &[], kcal: 15.0, protein: 1.1, carbs: 2.4, fat: 0.5, veg: true, cup_g: None, piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "lettuce", aliases: &["romaine", "romaine lettuce"], kcal: 15.0, protein: 1.4, carbs: 2.9, fat: 0.2, veg: true, cup_g: Some(47.0), piece_g: None, portion_g: 100.0, source: "USDA SR (recall)" },
    Food { name: "mushroom", aliases: &["button mushroom"], kcal: 22.0, protein: 3.1, carbs: 3.3, fat: 0.3, veg: true, cup_g: Some(70.0), piece_g: None, portion_g: 70.0, source: "USDA SR (recall)" },
];

pub const AMBIGUOUS: &[(&str, &str)] = &[
    ("rice", "say dry rice or cooked rice"),
    ("pasta", "say dry pasta or cooked pasta"),
    ("noodles", "say instant noodles, dry pasta or cooked pasta"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Unit {
    G,
    Oz,
    Cup,
    Tbsp,
    Tsp,
    Piece,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub food: &'static Food,
    pub grams: f64,
    pub unit: Unit,
    pub qty: f64,
    pub assumed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Unknown {
    pub text: String,
    pub hint: String,
}

#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub items: Vec<Item>,
    pub unknown: Vec<Unknown>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Macros {
    pub kcal: f64,
    pub protein: f64,
    pub carbs: f64,
    pub fat: f64,
}

impl fmt::Display for Unknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.text, self.hint)
    }
}

const PIECE_WORDS: &[&str] = &["piece", "pieces", "pc", "pcs", "fillet", "fillets", "clove", "cloves", "slice", "slices", "egg", "eggs", "head"];

/// A unit word and the factor it carries (`kg` is 1000 g, `oz` is 28.35 g, the rest are 1).
fn unit_word(raw: &str) -> Option<(Unit, f64)> {
    if raw == "T" {
        return Some((Unit::Tbsp, 1.0));
    }
    match raw.to_lowercase().as_str() {
        "g" | "gram" | "grams" => Some((Unit::G, 1.0)),
        "kg" => Some((Unit::G, 1000.0)),
        "oz" | "ounce" | "ounces" => Some((Unit::Oz, 28.35)),
        "cup" | "cups" | "c" => Some((Unit::Cup, 1.0)),
        "tbsp" | "tablespoon" | "tablespoons" => Some((Unit::Tbsp, 1.0)),
        "tsp" | "teaspoon" | "teaspoons" | "t" => Some((Unit::Tsp, 1.0)),
        w if PIECE_WORDS.contains(&w) => Some((Unit::Piece, 1.0)),
        _ => None,
    }
}

fn unit_name(u: Unit) -> &'static str {
    match u {
        Unit::G => "g",
        Unit::Oz => "oz",
        Unit::Cup => "cup",
        Unit::Tbsp => "tbsp",
        Unit::Tsp => "tsp",
        Unit::Piece => "piece",
    }
}

/// One vulgar-fraction character and its value.
fn vulgar(c: char) -> Option<f64> {
    Some(match c {
        '½' => 0.5,
        '¼' => 0.25,
        '¾' => 0.75,
        '⅕' => 0.2,
        '⅖' => 0.4,
        '⅗' => 0.6,
        '⅘' => 0.8,
        '⅙' => 1.0 / 6.0,
        '⅚' => 5.0 / 6.0,
        '⅐' => 1.0 / 7.0,
        '⅛' => 0.125,
        '⅜' => 0.375,
        '⅝' => 0.625,
        '⅞' => 0.875,
        '⅑' => 1.0 / 9.0,
        '⅒' => 0.1,
        '⅓' => 1.0 / 3.0,
        '⅔' => 2.0 / 3.0,
        _ => return None,
    })
}

/// Leading number: `100`, `0.5`, `.5`, `1/2`, `1 1/2`, `1½`, `1 ½`, `½`, or a range `1-2` read as its midpoint.
/// Returns the value and the rest. `None` when there is no number or it is not finite.
pub fn quantity(s: &str) -> Option<(f64, &str)> {
    let s = s.trim_start();
    if let Some(c) = s.chars().next() {
        if let Some(v) = vulgar(c) {
            return Some((v, &s[c.len_utf8()..]));
        }
    }
    let leading_dot = s.starts_with('.') && s[1..].starts_with(|c: char| c.is_ascii_digit());
    if !(leading_dot || s.starts_with(|c: char| c.is_ascii_digit())) {
        return None;
    }
    let num_end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let mut value: f64 = s[..num_end].parse().ok()?;
    let mut rest = &s[num_end..];
    let after_space = rest.trim_start_matches(' ');
    let spaced = rest.len() != after_space.len();
    match after_space.chars().next().and_then(|c| vulgar(c).map(|v| (v, c.len_utf8()))) {
        // "1½" and "1 ½".
        Some((v, n)) => {
            value += v;
            rest = &after_space[n..];
        }
        // Mixed number: "1 1/2".
        None if spaced && after_space.starts_with(|c: char| c.is_ascii_digit()) => {
            let w_end = after_space.find(|c: char| !(c.is_ascii_digit() || c == '/')).unwrap_or(after_space.len());
            if let Some((n, d)) = after_space[..w_end].split_once('/') {
                let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
                if d != 0.0 {
                    value += n / d;
                    rest = &after_space[w_end..];
                }
            }
        }
        None => {}
    }
    if let Some(r) = rest.strip_prefix('/') {
        let den_end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
        let den: f64 = r[..den_end].parse().ok()?;
        if den == 0.0 {
            return None;
        }
        value /= den;
        rest = &r[den_end..];
    }
    // Range "1-2" or "1–2": the midpoint, so "1-2 cups" is 1.5 cups.
    if let Some(r) = rest.strip_prefix(['-', '–']) {
        if let Some((hi, r2)) = quantity(r) {
            value = (value + hi) / 2.0;
            rest = r2;
        }
    }
    if !value.is_finite() {
        return None;
    }
    Some((value, rest))
}

/// `1,000` is one thousand; `1,5` stays two pieces (a decimal comma is not a separator).
fn join_thousands(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let digit = |i: usize| cs.get(i).is_some_and(|c| c.is_ascii_digit());
    let mut out = String::with_capacity(s.len());
    for i in 0..cs.len() {
        let thousands = cs[i] == ',' && i > 0 && digit(i - 1) && digit(i + 1) && digit(i + 2) && digit(i + 3) && !digit(i + 4);
        if !thousands {
            out.push(cs[i]);
        }
    }
    out
}

/// Splits on ` and ` in any letter case (` AND ` too), so no item is silently dropped.
fn split_and(p: &str) -> Vec<&str> {
    let lower = p.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut last = 0;
    for (i, _) in lower.match_indices(" and ") {
        out.push(&p[last..i]);
        last = i + " and ".len();
    }
    out.push(&p[last..]);
    out
}

/// Split a message into line items on `,`, `+`, `;`, newline and the word ` and `. Bullets (`- `, `* `) are dropped.
pub fn split_pieces(text: &str) -> Vec<String> {
    join_thousands(text)
        .split(|c| matches!(c, ',' | '+' | ';' | '\n'))
        .flat_map(split_and)
        .map(|p| {
            let p = p.trim();
            let p = p.strip_prefix("- ").or_else(|| p.strip_prefix("* ")).unwrap_or(p);
            p.trim()
        })
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

fn norm(s: &str) -> String {
    let cleaned: String = s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every alias (and food name, and ambiguous word) whose words all occur in `c`, in any order; the longest wins.
fn best_match(c: &str) -> Option<Result<&'static Food, Unknown>> {
    let words: Vec<&str> = c.split_whitespace().collect();
    let has = |a: &str| a.split_whitespace().all(|w| words.contains(&w));
    let mut hits: Vec<(usize, Result<&'static Food, Unknown>)> = Vec::new();
    for f in FOODS {
        for a in std::iter::once(f.name).chain(f.aliases.iter().copied()) {
            if has(a) {
                hits.push((a.len(), Ok(f)));
            }
        }
    }
    for (a, hint) in AMBIGUOUS {
        if has(a) {
            hits.push((a.len(), Err(Unknown { text: c.to_string(), hint: hint.to_string() })));
        }
    }
    hits.into_iter().max_by_key(|(len, _)| *len).map(|(_, r)| r)
}

/// Whether a message reads as food: it has an amount, or a word the table knows. Chat with neither is not a meal.
pub fn looks_like_food(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit() || vulgar(c).is_some()) || best_match(&norm(text)).is_some()
}

pub fn lookup(name: &str) -> Result<&'static Food, Unknown> {
    let n = norm(name);
    let n = n.strip_prefix("of ").unwrap_or(&n).to_string();
    let found = best_match(&n).or_else(|| n.strip_suffix('s').and_then(best_match));
    match found {
        Some(r) => r,
        None => Err(Unknown { text: n, hint: "not in the table; say grams or a name from the table".to_string() }),
    }
}

/// `12:30` is a clock time, not an amount; the colon in `rice:1/2 cup` is a separator.
fn is_clock(t: &str) -> bool {
    match t.split_once(':') {
        Some((h, m)) => (1..=2).contains(&h.len()) && m.len() == 2 && h.bytes().all(|b| b.is_ascii_digit()) && m.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Byte index of the first amount in `s`: a digit, a `.5`, or a vulgar fraction, at the start or after a space.
fn find_qty(s: &str) -> Option<usize> {
    s.char_indices()
        .find(|&(i, c)| {
            let starts = c.is_ascii_digit() || vulgar(c).is_some() || (c == '.' && s[i + 1..].starts_with(|d: char| d.is_ascii_digit()));
            starts && (i == 0 || s[..i].ends_with(char::is_whitespace))
        })
        .map(|(i, _)| i)
}

/// A unit word right after the amount, attached (`180g`) or spaced (`180 g`). Returns the raw word, the unit and the rest.
fn take_unit(rest: &str) -> (Option<(&str, Unit, f64)>, &str) {
    let alpha = rest.find(|c: char| !c.is_alphabetic()).unwrap_or(rest.len());
    if alpha > 0 {
        if let Some((u, f)) = unit_word(&rest[..alpha]) {
            return (Some((&rest[..alpha], u, f)), &rest[alpha..]);
        }
    }
    let r = rest.trim_start();
    let end = r.find(char::is_whitespace).unwrap_or(r.len());
    let word = &r[..end];
    match unit_word(word.trim_end_matches('.')) {
        Some((u, f)) => (Some((word, u, f)), &r[end..]),
        None => (None, rest),
    }
}

pub fn parse_line(piece: &str) -> Result<Item, Unknown> {
    let text = join_thousands(piece).split_whitespace().filter(|t| !is_clock(t)).collect::<Vec<_>>().join(" ").replace(':', " ");
    let whole = Unknown { text: piece.trim().to_string(), hint: String::new() };
    if text.split_whitespace().any(|t| t.len() > 1 && t.starts_with('-') && t[1..].starts_with(|c: char| c.is_ascii_digit())) {
        return Err(Unknown { hint: "negative amount; say how much you had".to_string(), ..whole });
    }
    let mut qty = None;
    let mut unit: Option<(Unit, f64)> = None;
    let mut food_text = text.trim().to_string();
    if let Some(i) = find_qty(&text) {
        let Some((q, rest)) = quantity(&text[i..]) else {
            return Err(Unknown { hint: "amount not readable; write it as 100, 1/2 or ½".to_string(), ..whole });
        };
        let before = text[..i].trim();
        let (taken, after) = take_unit(rest);
        food_text = format!("{before} {}", after.trim()).trim().to_string();
        qty = Some(q);
        if let Some((raw, u, f)) = taken {
            unit = Some((u, f));
            // "2 eggs": the piece word is the food itself.
            if food_text.is_empty() && u == Unit::Piece {
                food_text = raw.to_string();
            }
        }
    }
    if qty.is_some() && unit.is_none() {
        // "1 cod fillet" and "2 large egg": a piece word in the food name makes it a piece count.
        let words: Vec<&str> = food_text.split_whitespace().collect();
        if let Some(i) = words.iter().position(|w| PIECE_WORDS.contains(&w.to_lowercase().as_str())) {
            unit = Some((Unit::Piece, 1.0));
            let kept = words.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, w)| *w).collect::<Vec<_>>().join(" ");
            if !kept.is_empty() && lookup(&kept).is_ok() {
                food_text = kept;
            }
        }
    }
    let food = lookup(&food_text).map_err(|mut u| {
        u.text = food_text.clone();
        u
    })?;
    let (grams, qty_out, unit_out, assumed) = match qty {
        None => (food.portion_g, 1.0, Unit::G, true),
        Some(q) => {
            // A bare count of a food with a piece weight is pieces (up to 12); any other bare number is grams.
            let (u, f) = match unit {
                Some(x) => x,
                None if q <= 12.0 && food.piece_g.is_some() => (Unit::Piece, 1.0),
                None => (Unit::G, 1.0),
            };
            let per = match u {
                Unit::G | Unit::Oz => Some(f),
                Unit::Cup => food.cup_g,
                Unit::Tbsp => food.cup_g.map(|c| c / 16.0),
                Unit::Tsp => food.cup_g.map(|c| c / 48.0),
                Unit::Piece => food.piece_g,
            };
            match per {
                Some(p) => (q * p, q, u, false),
                None => {
                    let hint = format!("no {} weight for {}; give grams", unit_name(u), food.name);
                    return Err(Unknown { text: food_text, hint });
                }
            }
        }
    };
    Ok(Item { food, grams, unit: unit_out, qty: qty_out, assumed })
}

pub fn parse_meal(text: &str) -> Parsed {
    let mut p = Parsed::default();
    for piece in split_pieces(text) {
        match parse_line(&piece) {
            Ok(item) => p.items.push(item),
            Err(u) => p.unknown.push(u),
        }
    }
    p
}

pub fn macros(items: &[Item]) -> Macros {
    items.iter().fold(Macros::default(), |mut m, it| {
        let k = it.grams / 100.0;
        m.kcal += k * it.food.kcal;
        m.protein += k * it.food.protein;
        m.carbs += k * it.food.carbs;
        m.fat += k * it.food.fat;
        m
    })
}

pub fn has_veg(items: &[Item]) -> bool {
    items.iter().any(|i| i.food.veg && i.grams >= 30.0)
}

/// Typed items win over the recipe's line for the same food; the second value names the recipe foods overridden.
pub fn merge(typed: Parsed, recipe: Parsed) -> (Parsed, Vec<String>) {
    let typed_names: Vec<&'static str> = typed.items.iter().map(|i| i.food.name).collect();
    let mut overridden: Vec<String> = Vec::new();
    let mut items = typed.items;
    for it in recipe.items {
        if typed_names.contains(&it.food.name) {
            if !overridden.iter().any(|n| n == it.food.name) {
                overridden.push(it.food.name.to_string());
            }
        } else {
            items.push(it);
        }
    }
    let mut unknown = typed.unknown;
    unknown.extend(recipe.unknown);
    (Parsed { items, unknown }, overridden)
}

fn fmt_num(x: f64) -> String {
    let s = format!("{x:.1}");
    s.strip_suffix(".0").unwrap_or(&s).to_string()
}

fn fmt_qty(q: f64) -> String {
    for (v, s) in [(0.5, "½"), (0.25, "¼"), (0.75, "¾")] {
        if (q - v).abs() < 1e-9 {
            return s.to_string();
        }
    }
    fmt_num(q)
}

pub fn item_line(item: &Item) -> String {
    let g = fmt_num(item.grams);
    let kcal = (item.grams / 100.0 * item.food.kcal).round() as i64;
    let tail = if item.assumed {
        format!(" (default {g} g)")
    } else if item.unit == Unit::G {
        String::new()
    } else {
        format!(" ({} {})", fmt_qty(item.qty), unit_name(item.unit))
    };
    format!("{g} g {}{tail} · {kcal} kcal", item.food.name)
}
