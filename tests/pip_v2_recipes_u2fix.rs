//! U2 follow-ups: a range with a unit reads as its midpoint and is reported as assumed; an exact
//! recipe name plus a bare weight scales the recipe like the fuzzy path. Scratch `Private/Recipes`
//! fixtures (made-up folder, never the real vault).
use discord_inbox::nutrition;
use discord_inbox::recipes::{self, Recipe, Scaled};
use std::fs;
use std::path::PathBuf;

const SALMON: &str = "# Gochujang Honey Garlic Salmon\n\n## Per 100 g salmon portion\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n| Honey | 12 |\n";

const RICE: &str = "# Rice Bowl\n\n| | Recipe |\n|---|---|\n| Dry rice | 1–2 cups |\n| Brown rice | 1-2 cups |\n";

fn load(name: &str) -> Vec<Recipe> {
    let root = std::env::temp_dir().join(format!("pip-v2-u2fix-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Recipes");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gochujang Honey Garlic Salmon.md"), SALMON).unwrap();
    fs::write(dir.join("Rice Bowl.md"), RICE).unwrap();
    recipes::load(&dir)
}

fn named<'a>(rs: &'a [Recipe], name: &str) -> &'a Recipe {
    rs.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no recipe {name}"))
}

#[test]
fn en_dash_range_with_a_unit_reads_as_its_midpoint() {
    let rs = load("enrange");
    let bowl = named(&rs, "Rice Bowl");
    assert!(bowl.lines.contains(&"1.5 cups Dry rice".to_string()), "{:?}", bowl.lines);
    assert!(bowl.assumed.iter().any(|a| a.contains("Dry rice") && a.contains("midpoint")), "{:?}", bowl.assumed);
}

#[test]
fn hyphen_range_with_a_unit_reads_as_its_midpoint() {
    let rs = load("hyphrange");
    let bowl = named(&rs, "Rice Bowl");
    assert!(bowl.lines.contains(&"1.5 cups Brown rice".to_string()), "{:?}", bowl.lines);
    assert!(bowl.assumed.iter().any(|a| a.contains("Brown rice")), "{:?}", bowl.assumed);
}

#[test]
fn plain_grams_table_row_is_not_marked_assumed() {
    let rs = load("plain");
    let salmon = named(&rs, "Gochujang Honey Garlic Salmon");
    assert!(salmon.assumed.is_empty(), "{:?}", salmon.assumed);
}

#[test]
fn exact_name_with_a_bare_weight_scales_like_the_fuzzy_path() {
    let rs = load("exactweight");
    let hit = recipes::resolve("gochujang honey garlic salmon 150g", &rs).expect("hit");
    assert_eq!(hit.weight_g, Some(150.0));
    assert!(!hit.rest.contains("150"), "rest kept the weight: {:?}", hit.rest);
    let rest_parse = nutrition::parse_meal(&hit.rest);
    assert!(rest_parse.unknown.is_empty(), "weight read as unknown: {:?}", rest_parse.unknown);
    let (s, how) = recipes::scale(&hit, &nutrition::parse_meal(&hit.rest));
    assert!((s - 1.5).abs() < 1e-9, "got {s}");
    assert!(matches!(how, Scaled::Weight { food: "salmon", .. }), "{how:?}");
}

#[test]
fn exact_name_with_a_spaced_weight_is_also_taken() {
    let rs = load("spacedweight");
    let hit = recipes::resolve("gochujang honey garlic salmon 150 g", &rs).expect("hit");
    assert_eq!(hit.weight_g, Some(150.0));
    assert!(!hit.rest.contains("150"), "rest kept the weight: {:?}", hit.rest);
}

#[test]
fn weight_in_another_piece_is_not_the_recipe_weight() {
    let rs = load("otherpiece");
    let hit = recipes::resolve("gochujang honey garlic salmon, 50g honey", &rs).expect("hit");
    assert_eq!(hit.weight_g, None);
    assert!(hit.rest.contains("50g honey"), "typed honey weight was dropped: {:?}", hit.rest);
}
