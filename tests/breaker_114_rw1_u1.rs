//! Breaker round 3 (rework r1) on PR #114, area U1: quantity grammar and recipes.
//! Each test is one break; it asserts the behaviour the plan promises (use the typed amount, or refuse it).
//! Made-up fixtures only. Copied from the breaker comment on the PR (tests 1a, 1b, 1c, 2, 5, 7, 8)
//! plus a finding-8 test (number word in a recipe title).
use discord_inbox::nutrition;
use discord_inbox::recipes::{self, Hit, Recipe, Refused};

const TOFU: &str = "# Tofu 65\n\n| | g |\n|---|---|\n| Firm tofu | 100 |\n| Soy sauce | 11 |\n";
const POT_SALMON: &str = "# One Pot Salmon\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n";

fn recipe(stem: &str, text: &str) -> Recipe {
    recipes::parse_note(stem, text).expect("fixture has ingredient lines")
}

fn recipe_try<'a>(text: &str, rs: &'a [Recipe]) -> Result<Option<Hit<'a>>, Refused> {
    recipes::resolve_or_refuse(text, rs)
}

/// B1a: "tofu 65 1/2 kg" is half a kilo. Actual: HIT, one serving, the 1/2 kg vanishes.
#[test]
fn title_number_swallows_fraction_weight() {
    let rs = vec![recipe("Tofu 65", TOFU)];
    match recipe_try("tofu 65 1/2 kg", &rs) {
        Err(_) => {}
        Ok(Some(h)) => assert!(
            h.weight_g.is_some_and(|w| (w - 500.0).abs() < 1.0),
            "tofu 65 1/2 kg: expected 500 g or a refusal, got servings={} weight={:?} rest={:?}",
            h.servings, h.weight_g, h.rest
        ),
        Ok(None) => panic!("tofu 65 1/2 kg: no hit at all"),
    }
}

/// B1b: "tofu 65 1/2 cup" is a volume, which a recipe cannot scale by. Expected a refusal.
#[test]
fn title_number_swallows_fraction_cup() {
    let rs = vec![recipe("Tofu 65", TOFU)];
    assert!(recipe_try("tofu 65 1/2 cup", &rs).is_err(), "tofu 65 1/2 cup: expected a refusal, got a hit");
}

/// B1c: "tofu 65 1/2 servings" is half a serving. Actual: servings 1 (the 1/2 is lost).
#[test]
fn title_number_swallows_fraction_servings() {
    let rs = vec![recipe("Tofu 65", TOFU)];
    let h = recipe_try("tofu 65 1/2 servings", &rs).expect("should not refuse").expect("should hit");
    assert!((h.servings - 0.5).abs() < 1e-9, "tofu 65 1/2 servings: expected 0.5 servings, got {}", h.servings);
}

/// B2: "around 2 eggs" is two eggs. drop_times treats "around 2" as a clock time and drops both words.
#[test]
fn around_count_is_read_as_a_time() {
    match nutrition::parse_line("around 2 eggs") {
        Ok(i) => assert!((i.qty - 2.0).abs() < 1e-9, "around 2 eggs: expected qty 2, got qty {} ({} g, assumed={})", i.qty, i.grams, i.assumed),
        Err(u) => assert!(!u.hint.is_empty()),
    }
}

/// B5: a number word above twelve is silently ignored. Expected 13 g or a refusal.
#[test]
fn number_word_above_twelve_is_silently_dropped() {
    match nutrition::parse_line("salmon thirteen g") {
        Ok(i) => assert!(
            (i.grams - 13.0).abs() < 1e-6,
            "salmon thirteen g: expected 13 g or a refusal, got {} g (assumed={})", i.grams, i.assumed
        ),
        Err(_) => {}
    }
}

/// B7: "1,0000 g" is malformed. Actual: 1 g. Expected a refusal (or 10000 g, if read as that).
#[test]
fn malformed_thousands_group_becomes_one_gram() {
    match nutrition::parse_line("salmon 1,0000 g") {
        Ok(i) => assert!(
            (i.grams - 10000.0).abs() < 1e-6,
            "salmon 1,0000 g: expected a refusal or 10000 g, got {} g", i.grams
        ),
        Err(_) => {}
    }
}

/// B8: "salmon 0 g" logs a 0 g row with 0 kcal and no complaint. Expected a refusal.
#[test]
fn zero_gram_row_is_logged_silently() {
    match nutrition::parse_line("salmon 0 g") {
        Ok(i) => panic!("salmon 0 g: expected a refusal, logged {} g", i.grams),
        Err(_) => {}
    }
}

/// Finding 8: a number word in a recipe title ("One Pot Salmon") must not be read as a count.
/// Exact: "one pot salmon" is one serving, "one pot salmon 2" is two. Fuzzy: "one" in "pot salmon one" is not a count.
#[test]
fn one_pot_pasta_number_word_in_title() {
    let rs = vec![recipe("One Pot Salmon", POT_SALMON)];
    let h = recipe_try("one pot salmon", &rs).expect("should not refuse").expect("should hit");
    assert!((h.servings - 1.0).abs() < 1e-9 && !h.explicit, "one pot salmon: expected 1 serving, got {} (explicit={})", h.servings, h.explicit);
    let h = recipe_try("one pot salmon 2", &rs).expect("should not refuse").expect("should hit");
    assert!((h.servings - 2.0).abs() < 1e-9 && h.explicit, "one pot salmon 2: expected 2 servings, got {} (explicit={})", h.servings, h.explicit);
    let h = recipe_try("pot salmon one", &rs).expect("should not refuse").expect("should hit");
    assert!(!h.explicit, "pot salmon one: 'one' read as a count (servings {})", h.servings);
}
