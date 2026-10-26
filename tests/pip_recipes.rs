//! U2: recipe notes are read-only; their ingredient lines feed the nutrition parser. Fixtures live in a scratch `Private/Recipes`.
use discord_inbox::recipes::{self, Recipe};
use std::fs;
use std::path::PathBuf;

const SALMON: &str = "---\ntags:\n  - food\naliases:\n  - salmon bowl\n---\n# 🐟 Gochujang Honey Garlic Salmon\n\n## Per 100 g portion\n\n| | g |\n|---|---|\n| Salmon | 100 |\n| Soy sauce | 11 |\n";

const CUCUMBER: &str = "# Cucumber Side\n\n| | Recipe | Batch 1 (2026-10-22) |\n|---|---|---|\n| Cucumber | 300 g | **280 g** |\n| Sesame oil | 3-6 | 4 |\n| Rice vinegar | — | 15 g |\n";

const COD: &str = "# Baked Cod\n\n## Ingredients\n\n- 1 cod fillet, thawed\n- Neutral oil, a teaspoon\n";

const INDEX: &str = "# Recipes Index\n\n## Recipes\n\n- [[Baked Cod]]\n- [[Cucumber Side]]\n";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-recipes-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Recipes");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gochujang Honey Garlic Salmon.md"), SALMON).unwrap();
    fs::write(dir.join("Cucumber Side.md"), CUCUMBER).unwrap();
    fs::write(dir.join("Baked Cod.md"), COD).unwrap();
    fs::write(dir.join("Index.md"), INDEX).unwrap();
    dir
}

fn named<'a>(rs: &'a [Recipe], name: &str) -> &'a Recipe {
    rs.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no recipe {name}"))
}

#[test]
fn load_reads_three_recipes_and_skips_the_index() {
    let dir = scratch("load");
    let rs = recipes::load(&dir);
    assert_eq!(rs.len(), 3, "index has no ingredient lines");
    assert!(rs.iter().all(|r| r.name != "Index"));
}

#[test]
fn gram_table_under_a_per_100g_heading_gives_grams() {
    let dir = scratch("gtable");
    let rs = recipes::load(&dir);
    let r = named(&rs, "Gochujang Honey Garlic Salmon");
    assert!(r.lines.contains(&"100 g Salmon".to_string()), "{:?}", r.lines);
    assert!(r.lines.contains(&"11 g Soy sauce".to_string()), "{:?}", r.lines);
}

#[test]
fn recipe_column_is_chosen_over_the_batch_column() {
    let dir = scratch("column");
    let rs = recipes::load(&dir);
    let r = named(&rs, "Cucumber Side");
    assert!(r.lines.contains(&"300 g Cucumber".to_string()), "{:?}", r.lines);
    assert!(!r.lines.iter().any(|l| l.contains("280")), "batch column must not be used: {:?}", r.lines);
}

#[test]
fn dash_cell_is_skipped_and_reported() {
    let dir = scratch("dash");
    let rs = recipes::load(&dir);
    let r = named(&rs, "Cucumber Side");
    assert!(!r.lines.iter().any(|l| l.contains("Rice vinegar")), "{:?}", r.lines);
    assert!(r.skipped.iter().any(|s| s.contains("Rice vinegar")), "{:?}", r.skipped);
}

#[test]
fn range_cell_takes_its_first_number() {
    let dir = scratch("range");
    let rs = recipes::load(&dir);
    let r = named(&rs, "Cucumber Side");
    assert!(r.lines.contains(&"3 g Sesame oil".to_string()), "{:?}", r.lines);
}

#[test]
fn bullet_ingredients_cut_at_the_first_comma() {
    let dir = scratch("bullets");
    let rs = recipes::load(&dir);
    let r = named(&rs, "Baked Cod");
    assert!(r.lines.contains(&"1 cod fillet".to_string()), "{:?}", r.lines);
    assert!(r.lines.contains(&"Neutral oil".to_string()), "{:?}", r.lines);
}

#[test]
fn emoji_h1_and_alias_both_resolve() {
    let dir = scratch("names");
    let rs = recipes::load(&dir);
    assert!(recipes::resolve("gochujang honey garlic salmon", &rs).is_some());
    assert!(recipes::resolve("1 serving salmon bowl", &rs).is_some());
}

#[test]
fn serving_count_forms_scale_the_recipe() {
    let dir = scratch("scale");
    let rs = recipes::load(&dir);
    assert_eq!(recipes::resolve("2 servings Gochujang Honey Garlic Salmon", &rs).unwrap().servings, 2.0);
    assert_eq!(recipes::resolve("Cucumber Side x2", &rs).unwrap().servings, 2.0);
    assert_eq!(recipes::resolve("half salmon bowl", &rs).unwrap().servings, 0.5);
    assert_eq!(recipes::resolve("salmon bowl", &rs).unwrap().servings, 1.0);
}

#[test]
fn rest_drops_the_recipe_phrase_and_keeps_typed_overrides() {
    let dir = scratch("rest");
    let rs = recipes::load(&dir);
    let hit = recipes::resolve("1 serving salmon bowl, 150g salmon", &rs).unwrap();
    assert!(!hit.rest.contains("salmon bowl"), "{:?}", hit.rest);
    assert!(!hit.rest.contains("serving"), "{:?}", hit.rest);
    assert!(hit.rest.contains("150g salmon"), "{:?}", hit.rest);
}

#[test]
fn text_naming_no_recipe_resolves_to_nothing() {
    let dir = scratch("none");
    let rs = recipes::load(&dir);
    assert!(recipes::resolve("100g salmon, cabbage", &rs).is_none());
}

#[test]
fn recipes_dir_is_never_written() {
    let dir = scratch("readonly");
    let snap = |d: &PathBuf| -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = fs::read_dir(d)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.file_name().unwrap().to_string_lossy().into_owned(), fs::read(&p).unwrap())
            })
            .collect();
        v.sort();
        v
    };
    let before = snap(&dir);
    let rs = recipes::load(&dir);
    let _ = recipes::resolve("2 servings salmon bowl, 150g salmon", &rs);
    assert_eq!(before, snap(&dir), "recipes dir changed");
}
