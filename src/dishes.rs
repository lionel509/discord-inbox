//! #159: recipe notes with a kcal quoted in them, read by Pip as dish candidates. Skeleton: bodies are `todo!()` until U3.
use std::path::PathBuf;

/// One recipe note with the lines that quote kcal.
#[derive(Debug, Clone, PartialEq)]
pub struct Dish {
    pub note: String,
    pub lines: Vec<String>,
}

/// Dishes from `PIP_DISH_NOTES` paths: a folder gives its own `*.md` files, a file is one note.
pub fn load(paths: &[PathBuf]) -> Vec<Dish> {
    let _ = paths;
    todo!()
}

/// The kcal lines of a note body (front matter skipped), each cleaned and capped.
pub fn lines_of(text: &str) -> Vec<String> {
    let _ = text;
    todo!()
}

/// True when `kcal` is a number quoted on one of the dish's lines (a single number, or a range that holds it).
pub fn quoted(dish: &Dish, kcal: f64) -> bool {
    let _ = (dish, kcal);
    todo!()
}

/// The one dish whose note name matches `mark`; `None` for no match or several.
pub fn marked<'a>(mark: &str, dishes: &'a [Dish]) -> Option<&'a Dish> {
    let _ = (mark, dishes);
    todo!()
}
