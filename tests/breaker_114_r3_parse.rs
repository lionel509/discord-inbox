//! Breaker round 3 on PR #114 at head 1172b75 (parse + recipes): attacks only the round-2 fixes
//! 5c3c36b..1172b75 (the "no unit" refusal, brackets, fl oz, second weights, the main-protein gate,
//! the serving cap and the range refusal). Each test is one break. Made-up fixtures only.
use discord_inbox::nutrition;
use discord_inbox::recipes::{self, Recipe};

const SALMON: &str = "# Gochujang Honey Garlic Salmon\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n";
const BOWL: &str = "# Salmon Rice Bowl\n\n| | g |\n|---|---|\n| Cooked rice | 200 |\n| Salmon | 100 |\n| Soy sauce | 11 |\n";
const TOFU: &str = "# Tofu Stir Fry\n\n| | g |\n|---|---|\n| Broccoli | 200 |\n| Firm tofu | 100 |\n| Soy sauce | 11 |\n";
const FRIED: &str = "# Veggie Fried Rice\n\n| | g |\n|---|---|\n| Cooked rice | 300 |\n| Egg | 50 |\n| Peas | 50 |\n| Oil | 5 |\n";

fn recipe(stem: &str, text: &str) -> Recipe {
    recipes::parse_note(stem, text).expect("fixture has ingredient lines")
}

/// Total grams of the parsed items, and how many unknowns were reported.
fn grams(text: &str) -> (f64, usize) {
    let p = nutrition::parse_meal(text);
    (p.items.iter().map(|i| i.grams).sum(), p.unknown.len())
}

/// B1: "two eggs" is logged as one egg (50 g, assumed). The count word is dropped and no unknown is reported.
#[test]
fn two_eggs_is_not_one_egg() {
    let (g, unknown) = grams("two eggs");
    assert!(unknown > 0 || (g - 100.0).abs() < 1.0, "'two eggs' logged {g} g and {unknown} unknowns; expected 100 g or an unknown");
}

/// B2: "half an avocado" is logged as a whole avocado (150 g, the default portion). "half" is dropped silently.
#[test]
fn half_an_avocado_is_not_a_whole_one() {
    let (g, unknown) = grams("half an avocado");
    assert!(unknown > 0 || (g - 75.0).abs() < 1.0, "'half an avocado' logged {g} g and {unknown} unknowns; expected 75 g or an unknown");
}

/// B3: "salmon x2" is logged as 100 g (the default portion). The "x2" count is dropped silently,
/// while "salmon 2x" is refused as an unknown unit, so the two spellings disagree.
#[test]
fn salmon_x2_counts_both_portions() {
    let (g, unknown) = grams("salmon x2");
    assert!(unknown > 0 || (g - 200.0).abs() < 1.0, "'salmon x2' logged {g} g and {unknown} unknowns; expected 200 g or an unknown");
}

/// B4: a typed weight inside brackets is dropped. "1 banana (200 g)" logs 118 g and reports nothing,
/// because the bracket is read as a note and the 200 g is never counted or refused.
#[test]
fn typed_weight_in_brackets_is_not_dropped() {
    let (g, unknown) = grams("1 banana (200 g)");
    assert!(unknown > 0 || (g - 200.0).abs() < 1.0, "'1 banana (200 g)' logged {g} g and {unknown} unknowns; the typed 200 g was dropped");
}

/// B5: a percent fat label refuses a normal line. "1 cup 2% milk" is refused as "two amounts in one line",
/// because the "2" of "2%" is read as a second amount. Milk and yogurt labels hit this often.
#[test]
fn percent_fat_label_is_not_a_second_amount() {
    let p = nutrition::parse_meal("1 cup 2% milk");
    assert!(p.unknown.is_empty(), "'1 cup 2% milk' refused: {:?}", p.unknown.iter().map(|u| (&u.text, &u.hint)).collect::<Vec<_>>());
    let g: f64 = p.items.iter().map(|i| i.grams).sum();
    assert!((g - 244.0).abs() < 1.0, "'1 cup 2% milk' expected milk 244 g, got {g} g");
}

/// B6: a fuzzy hit reads "1 1/2 lb" as 1.5 servings. The fuzzy count reads "1" and "1/2" as a
/// serving count and ignores the pound. Expected: the typed 1 1/2 lb (680 g) scales the 100 g salmon by about 6.8.
#[test]
fn fuzzy_one_and_a_half_pounds_is_not_one_and_a_half_servings() {
    let all = vec![recipe("Gochujang Honey Garlic Salmon", SALMON)];
    let hit = recipes::resolve("honey garlic salmon 1 1/2 lb", &all).expect("fuzzy hit");
    let typed = nutrition::parse_meal(&hit.rest);
    let (ratio, how) = recipes::scale(&hit, &typed);
    assert!((ratio - 6.80).abs() < 0.05, "'1 1/2 lb' scaled the salmon by {ratio:.2} ({how:?}, explicit={}, servings={}); expected about 6.80", hit.explicit, hit.servings);
}

/// B7: the exact-name path ignores "1 1/2 lb" too. The pound stays in the rest, and the reply says
/// "Assumed 1 serving ... type the salmon weight", although the weight was typed.
#[test]
fn exact_name_one_and_a_half_pounds_is_read_as_a_weight() {
    let all = vec![recipe("Gochujang Honey Garlic Salmon", SALMON)];
    let hit = recipes::resolve("gochujang honey garlic salmon 1 1/2 lb", &all).expect("exact hit");
    let typed = nutrition::parse_meal(&hit.rest);
    let (ratio, how) = recipes::scale(&hit, &typed);
    assert!((ratio - 6.80).abs() < 0.05, "'1 1/2 lb' scaled the salmon by {ratio:.2} ({how:?}), rest={:?}; the typed weight was ignored", hit.rest);
}

/// B8: a cup measure becomes a serving count. "fried rice 1/2 cup" is an explicit 0.5 servings.
/// A cup is not a serving count, so this should be no hit or a non-explicit one.
#[test]
fn fried_rice_half_cup_is_not_half_a_serving() {
    let all = vec![recipe("Veggie Fried Rice", FRIED)];
    if let Some(hit) = recipes::resolve("fried rice 1/2 cup", &all) {
        assert!(!hit.explicit, "'fried rice 1/2 cup' read as an explicit {} servings; a cup is not a serving count", hit.servings);
    }
}

/// B9: the main-protein gate is keyed to the heaviest ingredient, not the protein. Tofu Stir Fry's heaviest
/// food is broccoli, which the title does not name, so no fuzzy hit is possible. The typo "tofu stir fri"
/// falls back to a silent 100 g tofu default and the recipe's broccoli, which is never logged.
#[test]
fn typo_of_recipe_with_unnamed_heaviest_food_still_matches() {
    let all = vec![recipe("Tofu Stir Fry", TOFU)];
    assert!(recipes::resolve("tofu stir fri", &all).is_some(), "'tofu stir fri' found no recipe; the typed tofu falls back to a default 100 g and the broccoli is never logged");
}

/// B10: "salmon bowl" finds no "Salmon Rice Bowl", because the gate needs the heaviest food (cooked rice) in
/// the message. A bowl named for its protein is refused.
#[test]
fn salmon_bowl_matches_salmon_rice_bowl() {
    let all = vec![recipe("Salmon Rice Bowl", BOWL)];
    assert!(recipes::resolve("salmon bowl", &all).is_some(), "'salmon bowl' found no recipe; the fuzzy gate wants cooked rice, the heaviest food, typed");
}
