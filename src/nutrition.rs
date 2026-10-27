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
    ("chicken", "say chicken breast or chicken thigh"),
];

/// Words that make a piece a different dish from the table food it names (`almond milk` is not milk).
const NOT_THE_SAME_FOOD: &[&str] = &[
    "almond", "oat", "soy", "coconut", "cashew", "chips", "crisps", "fries", "bread", "cake", "muffin", "cookie", "pie", "vinegar", "juice", "sauce", "powder", "flour", "oil", "butter", "jam", "syrup", "soup", "chocolate", "candy",
];

/// Unit-like words that are really a size, not a unit (`1 large egg`).
const SIZE_WORDS: &[&str] = &["large", "medium", "small", "big", "whole", "jumbo", "of"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Unit {
    G,
    Oz,
    Lb,
    Cup,
    Tbsp,
    Tsp,
    Piece,
    FlOz,
    Portion,
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

const PIECE_WORDS: &[&str] = &["piece", "pieces", "pc", "pcs", "fillet", "fillets", "clove", "cloves", "slice", "slices", "head", "heads"];

/// A unit word and the factor it carries (`kg` is 1000 g, `oz` is 28.35 g, the rest are 1).
fn unit_word(raw: &str) -> Option<(Unit, f64)> {
    if raw == "T" {
        return Some((Unit::Tbsp, 1.0));
    }
    match raw.to_lowercase().as_str() {
        "g" | "gram" | "grams" => Some((Unit::G, 1.0)),
        "kg" => Some((Unit::G, 1000.0)),
        "oz" | "ounce" | "ounces" => Some((Unit::Oz, 28.35)),
        "lb" | "lbs" | "pound" | "pounds" => Some((Unit::Lb, 453.592)),
        "cup" | "cups" | "c" => Some((Unit::Cup, 1.0)),
        "tbsp" | "tbs" | "tbsps" | "tablespoon" | "tablespoons" => Some((Unit::Tbsp, 1.0)),
        "tsp" | "teaspoon" | "teaspoons" | "t" => Some((Unit::Tsp, 1.0)),
        w if PIECE_WORDS.contains(&w) => Some((Unit::Piece, 1.0)),
        _ => None,
    }
}

fn unit_name(u: Unit) -> &'static str {
    match u {
        Unit::G => "g",
        Unit::Oz => "oz",
        Unit::Lb => "lb",
        Unit::Cup => "cup",
        Unit::Tbsp => "tbsp",
        Unit::Tsp => "tsp",
        Unit::Piece => "piece",
        Unit::FlOz => "fl oz",
        Unit::Portion => "portion",
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

/// `1,000` is one thousand. A comma between digits that is not a thousands group (`1,5`, `1,0000`) is kept:
/// `scan` refuses it, so it is never read as a decimal or as one gram.
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

/// Splits on `sep` in any letter case (` AND ` too), so no item is silently dropped.
/// Split `p` on " and ", except where a `keep` phrase (lowercase) covers the " and ".
fn split_and<'a>(p: &'a str, keep: &[String]) -> Vec<&'a str> {
    let lower = p.to_ascii_lowercase();
    let kept: Vec<(usize, usize)> = keep.iter().flat_map(|k| lower.match_indices(k.as_str()).map(move |(i, _)| (i, i + k.len()))).collect();
    let mut out = Vec::new();
    let mut last = 0;
    for (i, _) in lower.match_indices(" and ") {
        if kept.iter().any(|&(s, e)| s <= i && i < e) {
            continue;
        }
        out.push(&p[last..i]);
        last = i + 5;
    }
    out.push(&p[last..]);
    out
}

fn split_on<'a>(p: &'a str, sep: &str) -> Vec<&'a str> {
    let lower = p.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut last = 0;
    for (i, _) in lower.match_indices(sep) {
        out.push(&p[last..i]);
        last = i + sep.len();
    }
    out.push(&p[last..]);
    out
}

/// Split a message into line items on `,`, `+`, `;`, newline and the word ` and `. Bullets (`- `, `* `) are dropped.
pub fn split_pieces(text: &str) -> Vec<String> {
    split_pieces_keeping(text, &[])
}

/// `split_pieces`, except that an " and " inside one of the `keep` phrases (lowercase, ASCII) does not split.
/// Recipe titles with " and " in them (`Salt and Pepper Chicken`) stay whole this way.
pub fn split_pieces_keeping(text: &str, keep: &[String]) -> Vec<String> {
    // A comma between two digits is never a list separator: `1,5 kg` stays one piece, so `scan` can refuse it.
    let cs: Vec<char> = join_thousands(text).chars().collect();
    let mut parts: Vec<String> = vec![String::new()];
    for (i, &c) in cs.iter().enumerate() {
        let digit_comma = c == ',' && i > 0 && cs[i - 1].is_ascii_digit() && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit());
        if matches!(c, ',' | '+' | ';' | '\n') && !digit_comma {
            parts.push(String::new());
        } else if let Some(last) = parts.last_mut() {
            last.push(c);
        }
    }
    parts
        .iter()
        .flat_map(|p| split_and(p, keep))
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
    let has = |a: &str| a.split_whitespace().all(|w| has_word(&words, w));
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

/// Whether a message reads as food: a mass, a volume or a piece count, or a word the table knows.
/// Bare numbers and number words are not food (`no one`, `room 101`, `slept 8 hours` stay notes).
pub fn looks_like_food(text: &str) -> bool {
    scan(text).amounts.iter().any(|a| matches!(a.kind, Kind::Mass | Kind::Volume(_) | Kind::Piece { .. })) || best_match(&norm(text)).is_some()
}

pub fn lookup(name: &str) -> Result<&'static Food, Unknown> {
    let n = norm(name);
    let n = n.strip_prefix("of ").unwrap_or(&n).to_string();
    let found = best_match(&n).map(|r| (r, n.clone())).or_else(|| {
        let s = n.strip_suffix('s')?.to_string();
        best_match(&s).map(|r| (r, s))
    });
    match found {
        Some((Ok(food), used)) => {
            check_single_food(food, &used)?;
            Ok(food)
        }
        Some((Err(u), _)) => Err(u),
        None => Err(Unknown { text: n, hint: "not in the table; say grams or a name from the table".to_string() }),
    }
}

/// The table food is the whole piece: no other dish word (`banana bread`), and no second food (`eggs toast`).
fn check_single_food(food: &Food, used: &str) -> Result<(), Unknown> {
    let words: Vec<&str> = used.split_whitespace().collect();
    let alias = std::iter::once(food.name)
        .chain(food.aliases.iter().copied())
        .filter(|a| a.split_whitespace().all(|w| has_word(&words, w)))
        .max_by_key(|a| a.len())
        .unwrap_or(food.name);
    let alias_words: Vec<&str> = alias.split_whitespace().collect();
    if words.iter().any(|w| !alias_words.contains(w) && NOT_THE_SAME_FOOD.contains(w)) {
        return Err(Unknown { text: used.to_string(), hint: format!("'{used}' isn't in the table; it isn't plain {}", food.name) });
    }
    let rest: Vec<&str> = words.iter().copied().filter(|w| !alias_words.iter().any(|a| same_word(w, a))).collect();
    if !rest.is_empty() && best_match(&rest.join(" ")).is_some() {
        return Err(Unknown { text: used.to_string(), hint: "two foods in one line; put each on its own line".to_string() });
    }
    Ok(())
}

/// Whether `w` (a word after the amount) is a word of the food: its name or an alias, singular or plural.
fn is_food_word(w: &str, food: &Food) -> bool {
    std::iter::once(food.name).chain(food.aliases.iter().copied()).flat_map(str::split_whitespace).any(|c| c == w || w.strip_suffix('s') == Some(c) || c.strip_suffix('s') == Some(w))
}

/// The word right after an amount, as the parser sees it: a glued alphabetic run (`100ml`), else the first token.
fn word_after_amount(rest: &str) -> String {
    if rest.starts_with(char::is_alphabetic) {
        return rest.chars().take_while(|c| c.is_alphabetic()).collect::<String>().to_lowercase();
    }
    rest.split_whitespace().next().unwrap_or("").trim_end_matches(['.', ',', '!', '?', ')']).to_lowercase()
}

/// `12:30` is a clock time, not an amount; the colon in `rice:1/2 cup` is a separator.
fn is_clock(t: &str) -> bool {
    match t.split_once(':') {
        Some((h, m)) => (1..=2).contains(&h.len()) && m.len() == 2 && h.bytes().all(|b| b.is_ascii_digit()) && m.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Whether an amount (a digit, a `.5`, or a vulgar fraction) starts at byte `i` of `s`: at the start, after a space,
/// after an opening bracket, or after a dash that follows a word (`salmon-150g`).
fn amount_at(s: &str, i: usize, c: char) -> bool {
    let starts = c.is_ascii_digit() || vulgar(c).is_some() || (c == '.' && s[i + 1..].starts_with(|d: char| d.is_ascii_digit()));
    let before = &s[..i];
    let ok_before = before.is_empty()
        || before.ends_with(char::is_whitespace)
        || before.ends_with(['(', '['])
        || (before.ends_with('-') && before[..before.len() - 1].ends_with(char::is_alphabetic));
    starts && ok_before
}

/// A unit word right after the amount, attached (`180g`) or spaced (`180 g`). Returns the raw word, the unit and the rest.
fn take_unit(rest: &str) -> (Option<(&str, Unit, f64)>, &str) {
    if let Some(n) = fl_oz(rest.trim_start()) {
        let r = rest.trim_start();
        return (Some((&r[..n], Unit::FlOz, 1.0)), &r[n..]);
    }
    let alpha = rest.find(|c: char| !c.is_alphabetic()).unwrap_or(rest.len());
    if alpha > 0 {
        if let Some((u, f)) = unit_word(&rest[..alpha]) {
            return (Some((&rest[..alpha], u, f)), &rest[alpha..]);
        }
    }
    let r = rest.trim_start();
    let end = r.find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | ',' | ';')).unwrap_or(r.len());
    let word = &r[..end];
    match unit_word(word.trim_end_matches('.')) {
        Some((u, f)) => (Some((word, u, f)), &r[end..]),
        None => (None, rest),
    }
}

/// Whether a line word is `w`, or `w` with one trailing `s` (`eggs` for `egg`). The plural is on the line side only.
fn same_word(line_word: &str, w: &str) -> bool {
    line_word == w || line_word.strip_suffix('s') == Some(w)
}

/// Whether some word of the line is `w` (see `same_word`).
fn has_word(words: &[&str], w: &str) -> bool {
    words.iter().any(|lw| same_word(lw, w))
}

/// "fl oz", "fl. oz" or glued "floz" at the start of `r`: the bytes it takes, or None.
fn fl_oz(r: &str) -> Option<usize> {
    let lw = r.to_ascii_lowercase();
    let mut i = if lw.starts_with("floz") {
        4
    } else if lw.starts_with("fl") {
        let mut j = 2;
        if lw[j..].starts_with('.') {
            j += 1;
        }
        let ws = lw[j..].len() - lw[j..].trim_start().len();
        if ws == 0 || !lw[j + ws..].starts_with("oz") {
            return None;
        }
        j + ws + 2
    } else {
        return None;
    };
    if lw[i..].starts_with('.') {
        i += 1;
    }
    if lw[i..].starts_with(char::is_alphabetic) {
        return None;
    }
    Some(i)
}

/// "2 lb 4 oz" and "1 kg 200 g": the second weight (in the unit below) added to the first. Returns the total grams,
/// the rest after it, and the raw unit word of the second weight.
fn second_weight(q: f64, u: Unit, f: f64, after: &str) -> Option<(f64, &str, &str)> {
    let want = match (u, f) {
        (Unit::Lb, _) => Unit::Oz,
        (Unit::G, k) if k == 1000.0 => Unit::G,
        _ => return None,
    };
    let (q2, rest2) = quantity(after)?;
    let (taken, left) = take_unit(rest2);
    let (raw2, u2, f2) = taken?;
    if u2 != want {
        return None;
    }
    Some((q * f + q2 * f2, left, raw2))
}

fn is_hour(t: &str) -> bool {
    (1..=2).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()) && t.parse::<u32>().is_ok_and(|h| (1..=12).contains(&h))
}

fn is_ampm(t: &str) -> bool {
    matches!(t.to_ascii_lowercase().as_str(), "am" | "pm" | "a.m." | "p.m.")
}

/// `7pm`, `12:30pm`, `9a.m.`: a clock time glued to its am/pm.
fn glued_time(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    let Some(num) = ["a.m.", "p.m.", "am", "pm"].iter().find_map(|s| lower.strip_suffix(s)) else {
        return false;
    };
    match num.split_once(':') {
        Some((h, m)) => (1..=2).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_digit()) && m.len() == 2 && m.bytes().all(|b| b.is_ascii_digit()),
        None => is_hour(num),
    }
}

fn is_time_token(t: &str) -> bool {
    is_clock(t) || glued_time(t)
}

/// Drops the time-of-day phrases that are not amounts: `12:30`, `7pm`, `8 am`, and `at`/`@`/`around` in front of
/// one or of a bare whole hour with no unit after it (`salmon at 7`, `2 eggs at 8am`, `Had salmon at 7pm`).
fn drop_times(s: &str) -> String {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let n = toks.len();
    let mut drop = vec![false; n];
    for i in 0..n {
        if is_time_token(toks[i]) {
            drop[i] = true;
        } else if is_hour(toks[i]) && i + 1 < n && is_ampm(toks[i + 1]) {
            drop[i] = true;
            drop[i + 1] = true;
        }
    }
    for i in 0..n.saturating_sub(1) {
        if !matches!(toks[i].to_ascii_lowercase().as_str(), "at" | "@" | "around") {
            continue;
        }
        let next = i + 1;
        // A bare hour is a time only when it ends the phrase: the end, punctuation, another number or am/pm.
        // So `salmon at 7 150 g` drops the 7, and `around 2 eggs` keeps its 2.
        // A fraction is not another number: `around 1 1/2 cup` keeps its 1.
        let hour = toks[next].trim_end_matches(|c: char| c.is_ascii_punctuation());
        let punct_after = hour.len() < toks[next].len();
        let time_after = punct_after || toks.get(next + 1).is_none_or(|t| (t.starts_with(|c: char| c.is_ascii_digit()) && !t.contains('/')) || is_ampm(t) || t.bytes().all(|b| b.is_ascii_punctuation()));
        let bare_hour = !drop[next] && !hour.is_empty() && hour.len() <= 2 && hour.bytes().all(|b| b.is_ascii_digit()) && hour.parse::<u32>().is_ok_and(|h| (1..=24).contains(&h)) && time_after;
        if drop[next] || bare_hour {
            drop[i] = true;
            drop[next] |= bare_hour;
        }
    }
    toks.iter().zip(&drop).filter(|(_, d)| !**d).map(|(t, _)| *t).collect::<Vec<_>>().join(" ")
}

const TWO_AMOUNTS: &str = "two amounts in one line; send one";

/// Whether a bare count is typed first in its piece: nothing before it, or only `around` (`around 2 eggs`).
fn typed_first(before: &str) -> bool {
    let b = before.trim();
    b.is_empty() || b.eq_ignore_ascii_case("around")
}

/// The table entry for `per` of a unit (`cup`, `tbsp`, `piece`, `portion` ...), or None when the food has no such weight.
fn per_unit(food: &Food, u: Unit) -> Option<f64> {
    match u {
        Unit::Cup => food.cup_g,
        Unit::Tbsp => food.cup_g.map(|c| c / 16.0),
        Unit::Tsp => food.cup_g.map(|c| c / 48.0),
        Unit::Piece => food.piece_g,
        Unit::FlOz => food.cup_g.map(|c| c / 8.0),
        Unit::Portion => Some(food.portion_g),
        Unit::G | Unit::Oz | Unit::Lb => Some(1.0),
    }
}

/// One line item. Every amount `scan` finds is used (grams, a count or a portion), or the line is refused.
pub fn parse_line(piece: &str) -> Result<Item, Unknown> {
    let sc = scan(piece);
    let whole = |hint: &str| Unknown { text: piece.trim().to_string(), hint: hint.to_string() };
    // T1: a negative, unreadable or range amount is refused outright.
    if sc.amounts.iter().any(|a| a.kind == Kind::Negative) {
        return Err(whole("negative amount; say how much you had"));
    }
    if sc.amounts.iter().any(|a| a.kind == Kind::Unreadable) {
        return Err(whole("amount not readable; write it as 100, 1/2 or ½"));
    }
    if sc.amounts.iter().any(|a| a.kind == Kind::Range) {
        return Err(whole("a range; send one number"));
    }
    // T2: groups. A multiplier outside brackets, the amounts inside brackets, and the rest. One per group.
    let mults: Vec<&Amount> = sc.amounts.iter().filter(|a| a.kind == Kind::Multiplier && !a.bracketed).collect();
    let inside: Vec<&Amount> = sc.amounts.iter().filter(|a| a.bracketed).collect();
    let outside: Vec<&Amount> = sc.amounts.iter().filter(|a| !a.bracketed && a.kind != Kind::Multiplier).collect();
    if mults.len() > 1 || inside.len() > 1 || outside.len() > 1 {
        return Err(whole(TWO_AMOUNTS));
    }
    // T3: the main amount. A bracketed weight beats a loose count (`2 eggs (about 150 g)`); two weights must agree.
    let mut main: Option<Amount> = match (inside.first(), outside.first()) {
        (None, o) => o.map(|a| (*a).clone()),
        (Some(i), None) => Some((*i).clone()),
        (Some(i), Some(o)) => {
            if i.kind != Kind::Mass {
                return Err(whole(TWO_AMOUNTS));
            }
            if o.kind == Kind::Mass && (o.value - i.value).abs() > 0.1 * i.value {
                return Err(whole("two amounts disagree; send one"));
            }
            Some((*i).clone())
        }
    };
    // T4: the multiplier. A multiplier that is the main amount (`salmon (x2)`) is the multiplier.
    let mut m: Option<Amount> = mults.first().map(|a| (*a).clone());
    if main.as_ref().is_some_and(|a| a.kind == Kind::Multiplier) {
        if m.is_some() {
            return Err(whole(TWO_AMOUNTS));
        }
        m = main.take();
    }
    if let Some(x) = &m {
        if x.value == 0.0 || x.value > 12.0 {
            return Err(whole(&format!("x{} isn't a count I can use; send 1 to 12", fmt_num(x.value))));
        }
    }
    // T5: the food is the text left once every amount is taken out.
    let spans: Vec<(usize, usize)> = sc.amounts.iter().map(|a| (a.start, a.end)).collect();
    let mut food_text = strip_spans(&sc.text, &spans);
    if food_text.is_empty() {
        // "2 eggs" and "2 fillets": the piece word is the food itself.
        if let Some(a) = main.as_ref().filter(|a| matches!(a.kind, Kind::Piece { .. })) {
            food_text = a.unit.clone();
        }
    }
    // "1 cod fillet", "2 large egg": a piece word in the name makes a bare count a piece count.
    if let Some(a) = main.as_mut().filter(|a| a.kind == Kind::Bare) {
        let words: Vec<&str> = food_text.split_whitespace().collect();
        if let Some(i) = words.iter().position(|w| PIECE_WORDS.contains(&w.to_lowercase().as_str())) {
            a.kind = Kind::Piece { head: false };
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
    // T6: a word after a bare count that is not a unit, a piece word, a size or a word of the food.
    if let Some(a) = main.as_ref().filter(|a| a.kind == Kind::Bare) {
        let allowed = PIECE_WORDS.contains(&a.after.as_str()) || SIZE_WORDS.contains(&a.after.as_str()) || is_food_word(&a.after, food);
        if !a.after.is_empty() && !allowed {
            return Err(Unknown { text: food_text.clone(), hint: format!("unknown unit '{}'; give grams", a.after) });
        }
    }
    // T7: a head of a food with no piece weight.
    if main.as_ref().is_some_and(|a| a.kind == Kind::Piece { head: true }) && food.piece_g.is_none_or(|p| p < 200.0) {
        return Err(Unknown { text: food_text.clone(), hint: format!("a head of {} isn't in the table; give grams", food.name) });
    }
    // T8: grams. The multiplier scales the main amount; a portion is the food's portion_g.
    let mv = m.as_ref().map(|x| x.value);
    let portion = |c: f64| (c * food.portion_g, c, Unit::Portion, false);
    let (grams, qty, unit, assumed) = match main.as_ref() {
        None => match mv {
            None => (food.portion_g, 1.0, Unit::G, true),
            Some(c) => portion(c),
        },
        Some(a) if a.kind == Kind::Serving => portion(a.value * mv.unwrap_or(1.0)),
        Some(a) => match a.kind {
            Kind::Mass => match mv {
                Some(x) => (a.value * x, a.value * x, Unit::G, false),
                None => {
                    let (u, f) = unit_word(&a.unit).unwrap_or((Unit::G, 1.0));
                    (a.value, a.value / f, u, false)
                }
            },
            Kind::Volume(_) | Kind::Piece { .. } => {
                let u = match a.kind {
                    Kind::Volume(v) => v,
                    _ => Unit::Piece,
                };
                let qty = a.value * mv.unwrap_or(1.0);
                match per_unit(food, u) {
                    Some(p) => (qty * p, qty, u, false),
                    None => return Err(Unknown { text: food_text.clone(), hint: format!("no {} weight for {}; give grams", unit_name(u), food.name) }),
                }
            }
            Kind::Bare => {
                if mv.is_some() {
                    return Err(whole(TWO_AMOUNTS));
                }
                // A bare count of a food with a piece weight is pieces (up to 12), when it is typed first: "2 eggs".
                let (Some(p), true) = (food.piece_g, typed_first(&sc.text[..a.start])) else {
                    return Err(Unknown { text: food_text.clone(), hint: "no unit; add g, oz, lb, cup or pcs".to_string() });
                };
                if a.value > 12.0 {
                    return Err(Unknown { text: food_text.clone(), hint: format!("{0} pieces or {0} g? add g or pcs", fmt_num(a.value)) });
                }
                (a.value * p, a.value, Unit::Piece, false)
            }
            Kind::Multiplier | Kind::Range | Kind::Negative | Kind::Unreadable | Kind::Serving => return Err(whole(TWO_AMOUNTS)),
        },
    };
    // T9: a zero amount is refused (the recipe path does the same), so a typo never logs a 0 g row.
    if grams == 0.0 {
        return Err(Unknown { text: food_text, hint: "zero amount; nothing logged".to_string() });
    }
    // T9: a weight that is not finite, or over 10 kg, is refused.
    if !grams.is_finite() || grams > 10_000.0 {
        return Err(Unknown { text: food_text, hint: "over 10 kg; check the amount".to_string() });
    }
    Ok(Item { food, grams, unit, qty, assumed })
}

pub fn parse_meal(text: &str) -> Parsed {
    let mut p = Parsed::default();
    let pieces = split_pieces(text);
    // Typed meals also split on ` with ` ("salmon 150g with rice"); recipe lines go through here too, so this is the only place.
    for piece in pieces.iter().flat_map(|p| split_on(p, " with ")) {
        match parse_line(piece) {
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Mass,                 // value is grams: g/gram(s)/kg/oz/ounce(s)/lb/lbs/pound(s); "2 lb 4 oz" and "1 kg 200 g" summed into one
    Volume(Unit),         // value is a count of Cup/Tbsp/Tsp/FlOz (grams need the food's density)
    Piece { head: bool }, // explicit PIECE_WORDS unit (piece(s), pc(s), fillet(s), clove(s), slice(s), head(s)); head = raw unit is head/heads
    Serving,              // a number then serving/servings/portion/portions (a following "of" is consumed too)
    Multiplier,           // x2, X2, ×2, 2x, 2×, "x 2", "2 x"
    Bare,                 // a number (digits or a number word) with none of the above after it
    Range,                // 1-2, 1–2, 1 - 2, 1 – 2
    Negative,             // -100
    Unreadable,           // starts like a number but quantity() fails (1/0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Amount {
    pub value: f64,      // grams for Mass; the count otherwise; 0.0 for Range/Negative/Unreadable
    pub kind: Kind,
    pub start: usize,    // byte span in Scan.text: the number, its unit word(s), and any word the kind consumed ("an", "of", "x")
    pub end: usize,
    pub bracketed: bool, // the amount starts inside ( ) or [ ]
    pub word: bool,      // the number was a number word
    pub after: String,   // Bare only: word_after_amount(rest), lowercased; "" otherwise
    pub unit: String,    // raw unit text taken ("g", "cups", "fillet", "fl oz"); "" when none
}

#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub text: String,
    pub amounts: Vec<Amount>,
}

/// Every amount in one piece, read once. The only quantity reader: `parse_line` and both recipe paths use it.
pub fn scan(piece: &str) -> Scan {
    let text = drop_percent(&drop_times(&join_thousands(&piece.replace('~', " "))).replace(':', " "));
    let amounts = amounts_in(&text, None);
    Scan { text, amounts }
}

/// The amounts of `sc` that start outside `skip`, a byte span of `sc.text` (the recipe name the caller matched).
/// The name's own words are not read as amounts, so a title that ends in a number (`tofu 65 1/2 kg`) leaves the
/// fraction after it to be read on its own.
pub fn scan_outside(sc: &Scan, skip: (usize, usize)) -> Vec<Amount> {
    amounts_in(&sc.text, Some(skip))
}

/// Every amount in `text` whose start is not inside `skip`, read left to right.
fn amounts_in(text: &str, skip: Option<(usize, usize)>) -> Vec<Amount> {
    let mut amounts: Vec<Amount> = Vec::new();
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        let next = i + c.len_utf8();
        if skip.is_some_and(|(s, e)| i >= s && i < e) {
            i = next;
            continue;
        }
        match scan_at(text, i, c) {
            Some(mut a) => {
                a.bracketed = bracket_depth(&text[..a.start]) > 0;
                i = a.end.max(next);
                amounts.push(a);
            }
            None => i = next,
        }
    }
    amounts
}

/// `text` with the byte spans removed, ( ) [ ] turned into spaces, whitespace collapsed, trimmed.
pub fn strip_spans(text: &str, spans: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    for (idx, ch) in text.char_indices() {
        let hit = spans.iter().any(|&(s, e)| idx >= s && idx < e);
        out.push(if hit || matches!(ch, '(' | ')' | '[' | ']') { ' ' } else { ch });
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The amount that starts at byte `i` (`c` is the char there), or None. Rows S1–S5 of the grammar, in order.
fn scan_at(text: &str, i: usize, c: char) -> Option<Amount> {
    let at = |value: f64, kind: Kind, end: usize| Amount { value, kind, start: i, end, bracketed: false, word: false, after: String::new(), unit: String::new() };
    // S1: a negative amount, `-100`.
    if c == '-' && after_space_or_bracket(text, i) && text[i + 1..].starts_with(|d: char| d.is_ascii_digit()) {
        let end = match quantity(&text[i + 1..]) {
            Some((_, rest)) => text.len() - rest.len(),
            None => token_end(text, i),
        };
        return Some(at(0.0, Kind::Negative, end));
    }
    // S2 and S3: a multiplier written before its number, `x2` or `x 2`.
    if matches!(c, 'x' | 'X' | '×') && word_start(text, i) {
        let after_x = i + c.len_utf8();
        if text[after_x..].starts_with(|d: char| d.is_ascii_digit()) {
            return Some(match quantity(&text[after_x..]) {
                Some((q, rest)) => at(q, Kind::Multiplier, text.len() - rest.len()),
                None => at(0.0, Kind::Unreadable, token_end(text, i)),
            });
        }
        if text[after_x..].starts_with(char::is_whitespace) {
            let next = text[after_x..].trim_start();
            if next.starts_with(|d: char| d.is_ascii_digit()) {
                let start = text.len() - next.len();
                return Some(match quantity(next) {
                    Some((q, rest)) => at(q, Kind::Multiplier, text.len() - rest.len()),
                    None => at(0.0, Kind::Unreadable, token_end(text, start)),
                });
            }
        }
    }
    // S4: a number word (`two`, `half`) followed by a space, the end or punctuation. `half` and `quarter` take a following a/an.
    if c.is_alphabetic() && word_start(text, i) {
        let tok_end = text[i..].find(|ch: char| !ch.is_alphabetic()).map_or(text.len(), |p| i + p);
        let word = text[i..tok_end].to_lowercase();
        if let Some((v, takes_article)) = number_word(&word) {
            if boundary_after(text, tok_end) {
                let end = if takes_article { article_end(text, tok_end).unwrap_or(tok_end) } else { tok_end };
                return Some(unit_step(text, i, v, end, true));
            }
        }
        // `thirteen`, `twenty`, `hundred`: not read, so refused with the hint, never dropped.
        if is_big_number_word(&word) && boundary_after(text, tok_end) {
            return Some(at(0.0, Kind::Unreadable, tok_end));
        }
    }
    // S5: a digit, `.5` or a vulgar fraction, read by `quantity`.
    if amount_at(text, i, c) {
        return Some(match quantity(&text[i..]) {
            None => at(0.0, Kind::Unreadable, token_end(text, i)),
            Some((q, rest)) => {
                let end = text.len() - rest.len();
                // A comma between digits that is not a thousands group (`1,5`, `1,0000`) is refused.
                if text[end..].starts_with(',') && text[end + 1..].starts_with(|d: char| d.is_ascii_digit()) {
                    return Some(at(0.0, Kind::Unreadable, token_end(text, i)));
                }
                if text[i..end].contains(['-', '–']) {
                    at(0.0, Kind::Range, end)
                } else {
                    unit_step(text, i, q, end, false)
                }
            }
        });
    }
    None
}

/// What a number at `end` takes after it (the UNIT step, rows 1–6 of the grammar). `word` is set for a number word.
fn unit_step(text: &str, i: usize, q: f64, end: usize, word: bool) -> Amount {
    let rest = &text[end..];
    let mut a = Amount { value: q, kind: Kind::Bare, start: i, end, bracketed: false, word, after: String::new(), unit: String::new() };
    // 1. `2x`, `2×`: the x is glued to the number, then a non-alphanumeric char or the end.
    if let Some(r) = rest.strip_prefix(['x', 'X', '×']) {
        if r.chars().next().is_none_or(|ch| !ch.is_alphanumeric()) {
            return Amount { kind: Kind::Multiplier, end: text.len() - r.len(), ..a };
        }
    }
    // 2. `2 x`: a token that is exactly x, after spaces.
    let spaced = rest.trim_start();
    if let Some(r) = spaced.strip_prefix(['x', 'X', '×']) {
        if r.is_empty() || r.starts_with(char::is_whitespace) {
            return Amount { kind: Kind::Multiplier, end: text.len() - r.len(), ..a };
        }
    }
    // 3. `1 - 2`: a spaced range (an unspaced `1-2` is read as a range by the caller).
    if let Some(r) = spaced.strip_prefix(['-', '–']) {
        let r2 = r.trim_start();
        if r2.starts_with(|d: char| d.is_ascii_digit()) {
            let start = text.len() - r2.len();
            let stop = match quantity(r2) {
                Some((_, tail)) => text.len() - tail.len(),
                None => token_end(text, start),
            };
            return Amount { value: 0.0, kind: Kind::Range, end: stop, ..a };
        }
    }
    // 4. A unit word: a mass (a second weight in the unit below is summed into it), a volume or a piece word.
    let (taken, tail) = take_unit(rest);
    if let Some((raw, u, f)) = taken {
        let tail_end = text.len() - tail.len();
        match u {
            Unit::G | Unit::Oz | Unit::Lb => {
                let (value, stop, unit) = match second_weight(q, u, f, tail.trim_start()) {
                    Some((total, left, raw2)) => (total, text.len() - left.len(), format!("{raw} {raw2}")),
                    None => (q * f, tail_end, raw.to_string()),
                };
                return Amount { value, kind: Kind::Mass, end: stop, unit, ..a };
            }
            Unit::Cup | Unit::Tbsp | Unit::Tsp | Unit::FlOz => {
                return Amount { kind: Kind::Volume(u), end: tail_end, unit: raw.to_string(), ..a };
            }
            Unit::Piece => {
                let head = raw.eq_ignore_ascii_case("head") || raw.eq_ignore_ascii_case("heads");
                return Amount { kind: Kind::Piece { head }, end: tail_end, unit: raw.to_string(), ..a };
            }
            Unit::Portion => {}
        }
    }
    // 5. `2 servings`, `2 portions`, with a following `of` consumed too.
    let alpha = spaced.find(|ch: char| !ch.is_alphabetic()).unwrap_or(spaced.len());
    if matches!(spaced[..alpha].to_lowercase().as_str(), "serving" | "servings" | "portion" | "portions") {
        let mut stop = text.len() - spaced[alpha..].len();
        let after_word = text[stop..].trim_start();
        if after_word.get(..2).is_some_and(|w| w.eq_ignore_ascii_case("of")) && after_word[2..].chars().next().is_none_or(|ch| !ch.is_alphanumeric()) {
            stop = text.len() - after_word[2..].len();
        }
        return Amount { kind: Kind::Serving, end: stop, ..a };
    }
    // 6. A bare count. `after` is the word that follows, for the unit check.
    a.after = word_after_amount(rest);
    a
}

/// A number word and whether a following `a`/`an` joins it (`half an avocado`).
fn number_word(w: &str) -> Option<(f64, bool)> {
    const WORDS: [&str; 12] = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"];
    if let Some(p) = WORDS.iter().position(|x| *x == w) {
        return Some(((p + 1) as f64, false));
    }
    match w {
        "half" => Some((0.5, true)),
        "quarter" => Some((0.25, true)),
        _ => None,
    }
}

/// Number words the grammar does not read: thirteen to nineteen, the tens and `hundred`.
fn is_big_number_word(w: &str) -> bool {
    matches!(
        w,
        "thirteen" | "fourteen" | "fifteen" | "sixteen" | "seventeen" | "eighteen" | "nineteen" | "twenty" | "thirty" | "forty" | "fifty" | "sixty" | "seventy" | "eighty" | "ninety" | "hundred"
    )
}

/// The end of a whole word `a` or `an` after `tok_end`, if one follows after spaces.
fn article_end(text: &str, tok_end: usize) -> Option<usize> {
    let rest = &text[tok_end..];
    let spaced = rest.trim_start();
    if spaced.len() == rest.len() {
        return None;
    }
    let alpha = spaced.find(|ch: char| !ch.is_alphabetic()).unwrap_or(spaced.len());
    let end = text.len() - spaced.len() + alpha;
    (matches!(spaced[..alpha].to_lowercase().as_str(), "a" | "an") && boundary_after(text, end)).then_some(end)
}

/// Whether byte `j` is a word boundary: the end, a space, or one of `,.;:!?)]`.
fn boundary_after(text: &str, j: usize) -> bool {
    text[j..].chars().next().is_none_or(|ch| ch.is_whitespace() || ",.;:!?)]".contains(ch))
}

/// Whether byte `i` starts a word: the start, or a char before it that is not a letter or digit.
fn word_start(text: &str, i: usize) -> bool {
    text[..i].chars().next_back().is_none_or(|p| !p.is_alphanumeric())
}

/// Whether byte `i` starts a token after a space, an opening bracket or the start.
fn after_space_or_bracket(text: &str, i: usize) -> bool {
    text[..i].chars().next_back().is_none_or(|p| p.is_whitespace() || p == '(' || p == '[')
}

/// Byte index where the whitespace-delimited token that starts at `i` ends.
fn token_end(text: &str, i: usize) -> usize {
    text[i..].find(char::is_whitespace).map_or(text.len(), |p| i + p)
}

/// How many brackets are open at the end of `s`.
fn bracket_depth(s: &str) -> usize {
    s.chars().fold(0usize, |d, c| match c {
        '(' | '[' => d + 1,
        ')' | ']' => d.saturating_sub(1),
        _ => d,
    })
}

/// Removes `2%`, `1.5%` style fat labels (a whitespace token, with an optional trailing `,.;`) and rejoins the text.
fn drop_percent(text: &str) -> String {
    text.split_whitespace().filter(|t| !is_percent_label(t)).collect::<Vec<_>>().join(" ")
}

fn is_percent_label(t: &str) -> bool {
    t.trim_end_matches([',', '.', ';']).strip_suffix('%').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.'))
}
