//! F2 (PR #114 round 1): breaker tests for the recipe fuzzy pass, zero counts, mixed counts and
//! unitless table ranges. Copied verbatim from the parse+recipes breaker comment (made-up fixtures).
use discord_inbox::recipes;

const FIXTURE: &str = "---\naliases: [Garlic Glazed Fish]\n---\n# Honey Garlic Salmon\n\n## Ingredients\n- 150 g salmon\n- 1 tbsp honey\n- 1 clove garlic\n";

fn fixture() -> recipes::Recipe {
    recipes::parse_note("Honey Garlic Salmon", FIXTURE).expect("fixture parses")
}

// 1. "1,5" is split into "1" and "5 cups oats": 405 g of oats instead of 1.5 cups (121.5 g).

// 9. "chicken" is not in the salmon recipe, yet the fuzzy pass (2 of 3 name words) hits the salmon recipe.
#[test]
fn fuzzy_recipe_hit_does_not_swallow_unknown_food() {
    let r = [fixture()];
    let hit = recipes::resolve("honey garlic chicken 150g", &r);
    assert!(hit.is_none(), "chicken silently matched the salmon recipe: {:?}", hit.map(|h| h.matched));
}

// 10. "x 0" with a space is not read as zero servings: the recipe is logged as one serving.
#[test]
fn spaced_zero_count_is_zero_servings() {
    let r = [fixture()];
    let hit = recipes::resolve("honey garlic salmon x 0", &r);
    assert!(hit.is_none(), "x 0 became {} serving(s)", hit.map(|h| h.servings).unwrap_or(-1.0));
}

// 12. A table range without a unit (100-150) reads as its low end, with no assumed note.
#[test]
fn table_range_without_unit_is_flagged() {
    let text = "---\n---\n# Quiet Fish\n\n| Ingredient | Amount |\n|---|---|\n| salmon | 100-150 |\n";
    let r = recipes::parse_note("Quiet Fish", text).expect("fixture parses");
    assert!(!r.assumed.is_empty() || !r.skipped.is_empty(), "100-150 read silently as {:?}", r.lines);
}

// 14. A serving count written after the name: "1 1/2 servings" reads as 1 serving, rest "1/2 servings".
#[test]
fn mixed_serving_count_after_name_is_one_and_a_half() {
    let r = [fixture()];
    let hit = recipes::resolve("honey garlic salmon 1 1/2 servings", &r).expect("exact name hit");
    assert!((hit.servings - 1.5).abs() < 1e-9, "1 1/2 servings became {} (rest {:?})", hit.servings, hit.rest);
}
