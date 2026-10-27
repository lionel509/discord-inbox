//! U2: fuzzy recipe names, weight-based scaling, and the scratch `Private/Recipes` fixtures (made-up folder, never the real vault).
use discord_inbox::nutrition;
use discord_inbox::recipes::{self, Recipe, Scaled};
use std::fs;
use std::path::PathBuf;

const SALMON: &str = "---\ntags:\n  - food\n---\n# 🐟 Gochujang Honey Garlic Salmon\n\n## Per 100 g salmon portion\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n| Honey | 12 |\n| Gochujang | 4 |\n| Garlic | 3 |\n| Sesame oil | 2 |\n| Rice vinegar | 3 |\n| Scallion | 9 |\n";

const CHICKEN: &str = "# Honey Garlic Chicken\n\n## Ingredients\n\n- 150 g chicken thigh\n- 15 g honey\n- 3 g garlic\n";

const COD: &str = "# Baked Cod\n\n## Ingredients\n\n- 1 cod fillet, thawed overnight\n- Neutral oil, a teaspoon\n";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-v2-recipes-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Recipes");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gochujang Honey Garlic Salmon.md"), SALMON).unwrap();
    fs::write(dir.join("Honey Garlic Chicken.md"), CHICKEN).unwrap();
    fs::write(dir.join("Baked Cod.md"), COD).unwrap();
    dir
}

fn load(name: &str) -> Vec<Recipe> {
    recipes::load(&scratch(name))
}

fn named<'a>(rs: &'a [Recipe], name: &str) -> &'a Recipe {
    rs.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no recipe {name}"))
}

#[test]
fn misspelt_fuzzy_name_resolves_to_the_salmon_recipe() {
    let rs = load("fuzzy");
    let hit = recipes::resolve("the samon made with the hot honey garlic glaze", &rs).expect("hit");
    assert_eq!(hit.recipe.name, "Gochujang Honey Garlic Salmon");
    assert!(hit.fuzzy);
    assert!(!hit.explicit);
}

#[test]
fn exact_chicken_name_resolves_to_chicken() {
    let rs = load("chicken");
    let hit = recipes::resolve("honey garlic chicken", &rs).expect("hit");
    assert_eq!(hit.recipe.name, "Honey Garlic Chicken");
}

#[test]
fn single_word_or_plain_food_lines_are_not_recipe_hits() {
    let rs = load("none");
    assert!(recipes::resolve("salmon 100 grams", &rs).is_none());
    assert!(recipes::resolve("honey 20g", &rs).is_none());
    assert!(recipes::resolve("100g salmon, cabbage", &rs).is_none());
}

#[test]
fn typed_weight_of_the_main_ingredient_is_read_from_the_hit() {
    let rs = load("weight");
    let hit = recipes::resolve("honey garlic salmon 150g", &rs).expect("hit");
    assert_eq!(hit.weight_g, Some(150.0));
    let typed = nutrition::parse_meal("honey garlic salmon 150g");
    let (s, how) = recipes::scale(&hit, &typed);
    assert!((s - 1.5).abs() < 1e-9);
    assert!(matches!(how, Scaled::Weight { food: "salmon", .. }), "{how:?}");
}

#[test]
fn typed_salmon_line_scales_the_whole_recipe_when_no_count_is_given() {
    let rs = load("typed");
    let hit = recipes::resolve("gochujang honey garlic salmon", &rs).expect("hit");
    let typed = nutrition::parse_meal("salmon 150 g");
    let (s, how) = recipes::scale(&hit, &typed);
    assert!((s - 1.5).abs() < 1e-9, "got {s}");
    assert!(matches!(how, Scaled::Typed { food: "salmon", .. }), "{how:?}");
}

#[test]
fn nothing_typed_assumes_one_serving() {
    let rs = load("assumed");
    let hit = recipes::resolve("gochujang honey garlic salmon", &rs).expect("hit");
    let (s, how) = recipes::scale(&hit, &nutrition::Parsed::default());
    assert!((s - 1.0).abs() < 1e-9);
    assert_eq!(how, Scaled::Assumed);
}

#[test]
fn explicit_count_wins_and_is_counted() {
    let rs = load("count");
    let hit = recipes::resolve("2 x gochujang honey garlic salmon", &rs).expect("hit");
    assert!(hit.explicit);
    let (s, how) = recipes::scale(&hit, &nutrition::parse_meal("salmon 150 g"));
    assert!((s - 2.0).abs() < 1e-9);
    assert_eq!(how, Scaled::Count);
}

#[test]
fn the_recipe_table_loads_with_its_eight_rows() {
    let rs = load("table");
    let salmon = named(&rs, "Gochujang Honey Garlic Salmon");
    assert_eq!(salmon.lines.len(), 8, "{:?}", salmon.lines);
}
