//! F2 (PR #114 round 1): typo tolerance for the fuzzy recipe pass needs two words, and a weight
//! after a misspelt name still scales the salmon recipe. Scratch `Private/Recipes` fixtures
//! (made-up folder, never the real vault).
use discord_inbox::recipes::{self, Recipe};
use std::fs;
use std::path::PathBuf;

const SALMON: &str = "---\ntags:\n  - food\n---\n# 🐟 Gochujang Honey Garlic Salmon\n\n## Per 100 g salmon portion\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n| Honey | 12 |\n| Gochujang | 4 |\n| Garlic | 3 |\n| Sesame oil | 2 |\n| Rice vinegar | 3 |\n| Scallion | 9 |\n";

const CHICKEN: &str = "# Honey Garlic Chicken\n\n## Ingredients\n\n- 150 g chicken thigh\n- 15 g honey\n- 3 g garlic\n";

const COD: &str = "# Baked Cod\n\n## Ingredients\n\n- 1 cod fillet, thawed overnight\n- Neutral oil, a teaspoon\n";

fn load(name: &str) -> Vec<Recipe> {
    let root = std::env::temp_dir().join(format!("pip-v2-fix-recipes-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Recipes");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gochujang Honey Garlic Salmon.md"), SALMON).unwrap();
    fs::write(dir.join("Honey Garlic Chicken.md"), CHICKEN).unwrap();
    fs::write(dir.join("Baked Cod.md"), COD).unwrap();
    recipes::load(&dir)
}

#[test]
fn misspelt_name_with_a_weight_is_the_salmon_recipe_at_that_weight() {
    let rs = load("typo");
    let hit = recipes::resolve("gochujng samon 150g", &rs).expect("hit");
    assert_eq!(hit.recipe.name, "Gochujang Honey Garlic Salmon");
    assert!(hit.fuzzy);
    assert_eq!(hit.weight_g, Some(150.0));
}

#[test]
fn one_matching_word_is_not_a_recipe_hit() {
    let rs = load("one");
    assert!(recipes::resolve("cod 100g", &rs).is_none());
}
