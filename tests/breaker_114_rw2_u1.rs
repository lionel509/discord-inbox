//! Breaker round 4 (rework r2) on PR #114, area U1: quantity grammar and recipe names.
//! Copied from the breaker comment on the PR (B1, B2, B3 from comment 6430591142), plus the
//! finding-2 pins and guards from the reviewer's round-3 fix list. Made-up fixtures only.
use discord_inbox::nutrition;
use discord_inbox::recipes::{self, Hit, Recipe, Refused};

const SALT_CHICKEN: &str = "# Salt and Pepper Chicken\n\n| | g |\n|---|---|\n| Chicken thigh | 100 |\n| Salt | 2 |\n";

fn grams(piece: &str) -> Result<f64, String> {
    nutrition::parse_line(piece).map(|it| it.grams).map_err(|u| u.hint)
}

fn recipe(stem: &str, text: &str) -> Recipe {
    recipes::parse_note(stem, text).expect("fixture has ingredient lines")
}

fn recipe_try<'a>(text: &str, rs: &'a [Recipe]) -> Result<Option<Hit<'a>>, Refused> {
    recipes::resolve_or_refuse(text, rs)
}

/// B1: `around` before a mixed number. `around 1 1/2 cup` must be 1.5 cup (277.5 g, as `1 1/2 cup` is).
/// `drop_times` reads the 1 as a bare hour because the next token starts with a digit, so only the 1/2 cup is left.
#[test]
fn around_mixed_number_is_not_read_as_a_time() {
    let plain = grams("dry rice 1 1/2 cup").expect("plain 1 1/2 cup must parse");
    let around = grams("dry rice around 1 1/2 cup");
    match around {
        Err(_) => {}
        Ok(g) => assert!((g - plain).abs() < 0.5, "dry rice around 1 1/2 cup: expected {plain} g (as without around), got {g} g"),
    }
}

/// B2: `at` before a mixed number, same mechanism. `at 2 1/2 tbsp` must keep the 2.
#[test]
fn at_mixed_number_keeps_the_whole_amount() {
    let plain = grams("dry rice 2 1/2 tbsp").expect("plain 2 1/2 tbsp must parse");
    match grams("dry rice at 2 1/2 tbsp") {
        Err(_) => {}
        Ok(g) => assert!((g - plain).abs() < 0.5, "dry rice at 2 1/2 tbsp: expected {plain} g, got {g} g"),
    }
}

/// B3: `around` before a count with a fraction. `around 2 1/2 eggs` must be 2.5 eggs (125 g), not half an egg.
#[test]
fn around_count_with_fraction_keeps_the_whole_count() {
    match grams("around 2 1/2 eggs") {
        Err(_) => {}
        Ok(g) => assert!((g - 125.0).abs() < 1.0, "around 2 1/2 eggs: expected 125 g, got {g} g"),
    }
}

/// Finding 2: a recipe title with " and " in it must still match exactly, as it did before the " and " split.
#[test]
fn and_title_matches_exactly() {
    let rs = vec![recipe("Salt and Pepper Chicken", SALT_CHICKEN)];
    match recipe_try("salt and pepper chicken", &rs) {
        Ok(Some(h)) => {
            assert!(h.explicit == false && h.servings == 1.0, "salt and pepper chicken: expected 1 serving, got {} explicit={}", h.servings, h.explicit);
            assert!(h.rest.is_empty(), "salt and pepper chicken: expected no other items, got rest={:?}", h.rest);
            assert!(!h.fuzzy, "salt and pepper chicken: expected an exact hit, got a fuzzy one");
        }
        other => panic!("salt and pepper chicken: expected an exact hit, got {:?}", other.map(|o| o.map(|h| h.servings))),
    }
}

/// Finding 2: the weight after an " and " title survives.
#[test]
fn and_title_keeps_its_weight() {
    let rs = vec![recipe("Salt and Pepper Chicken", SALT_CHICKEN)];
    match recipe_try("salt and pepper chicken 150g", &rs) {
        Ok(Some(h)) => {
            assert!(!h.fuzzy, "salt and pepper chicken 150g: expected an exact hit, got a fuzzy one (rest={:?})", h.rest);
            assert!(
                h.weight_g.is_some_and(|w| (w - 150.0).abs() < 0.5),
                "salt and pepper chicken 150g: expected weight 150 g, got weight={:?} rest={:?}",
                h.weight_g, h.rest
            );
        }
        other => panic!("salt and pepper chicken 150g: expected a hit, got {:?}", other.map(|o| o.map(|h| h.servings))),
    }
}

/// Finding 2: the count typed before an " and " title is kept.
#[test]
fn and_title_keeps_its_typed_count() {
    let rs = vec![recipe("Salt and Pepper Chicken", SALT_CHICKEN)];
    match recipe_try("2 salt and pepper chicken", &rs) {
        Ok(Some(h)) => {
            assert!(h.servings == 2.0 && h.explicit, "2 salt and pepper chicken: expected 2 servings explicit, got servings={} explicit={}", h.servings, h.explicit);
        }
        other => panic!("2 salt and pepper chicken: expected a hit, got {:?}", other.map(|o| o.map(|h| h.servings))),
    }
}

/// Guard (green before and after): an " and " after the title still separates the next item.
#[test]
fn and_after_and_title_still_separates_items() {
    let rs = vec![recipe("Salt and Pepper Chicken", SALT_CHICKEN)];
    match recipe_try("salt and pepper chicken and 2 eggs", &rs) {
        Ok(Some(h)) => {
            assert!(h.rest.contains("2 eggs"), "salt and pepper chicken and 2 eggs: expected 2 eggs left as an item, got rest={:?}", h.rest);
            assert!(h.servings == 1.0, "salt and pepper chicken and 2 eggs: expected 1 serving of the recipe, got {}", h.servings);
        }
        other => panic!("salt and pepper chicken and 2 eggs: expected a hit, got {:?}", other.map(|o| o.map(|h| h.servings))),
    }
}

/// Guard (green before and after): a non-recipe line with " and " still splits into two items.
#[test]
fn and_between_table_items_still_splits() {
    let rs = vec![recipe("Salt and Pepper Chicken", SALT_CHICKEN)];
    assert_eq!(nutrition::split_pieces("salmon 100g and 2 eggs").len(), 2, "salmon 100g and 2 eggs: expected two items");
    assert!(matches!(recipe_try("salmon 100g and 2 eggs", &rs), Ok(None)), "salmon 100g and 2 eggs: expected no recipe hit");
}
