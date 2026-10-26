//! Read-only recipe notes to ingredient lines (U2). Stub: every function is todo!().
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

pub fn load(_dir: &Path) -> Vec<Recipe> {
    todo!()
}
pub fn parse_note(_stem: &str, _text: &str) -> Option<Recipe> {
    todo!()
}
pub fn resolve<'a>(_text: &str, _recipes: &'a [Recipe]) -> Option<Hit<'a>> {
    todo!()
}
pub fn ingredient_pieces(_hit: &Hit) -> Vec<String> {
    todo!()
}
