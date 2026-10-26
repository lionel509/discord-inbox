//! Local nutrition table and line parser (U1). Stub: every function is todo!().
use std::fmt;

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

pub const FOODS: &[Food] = &[];
pub const AMBIGUOUS: &[(&str, &str)] = &[];

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

pub fn quantity(_s: &str) -> Option<(f64, &str)> {
    todo!()
}
pub fn split_pieces(_text: &str) -> Vec<String> {
    todo!()
}
pub fn lookup(_name: &str) -> Result<&'static Food, Unknown> {
    todo!()
}
pub fn parse_line(_piece: &str) -> Result<Item, Unknown> {
    todo!()
}
pub fn parse_meal(_text: &str) -> Parsed {
    todo!()
}
pub fn macros(_items: &[Item]) -> Macros {
    todo!()
}
pub fn has_veg(_items: &[Item]) -> bool {
    todo!()
}
pub fn merge(_typed: Parsed, _recipe: Parsed) -> (Parsed, Vec<String>) {
    todo!()
}
pub fn item_line(_item: &Item) -> String {
    todo!()
}
