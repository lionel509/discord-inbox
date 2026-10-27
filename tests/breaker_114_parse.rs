//! Breaker tests for PR #114 (area: parse + recipes). Each test encodes what the plan promises:
//! no guessing, unknown foods reported, kcal never silently wrong. Made-up recipe fixtures only.
use discord_inbox::nutrition::{self, Item};

fn total(items: &[Item]) -> f64 {
    items.iter().map(|i| i.grams).sum()
}

// 1. "1,5" is split into "1" and "5 cups oats": 405 g of oats instead of 1.5 cups (121.5 g).
#[test]
fn decimal_comma_quantity_is_not_multiplied() {
    let p = nutrition::parse_meal("1,5 cups oats");
    assert!(p.items.is_empty() || (total(&p.items) - 121.5).abs() < 1.0, "items {:?}", p.items.iter().map(|i| (i.food.name, i.grams)).collect::<Vec<_>>());
}

// 2. "lb" is not a unit: one pound of chicken is logged as 1 gram.
#[test]
fn pound_is_not_one_gram() {
    if let Ok(it) = nutrition::parse_line("1 lb chicken breast") {
        assert!(it.grams > 400.0, "1 lb chicken breast became {} g", it.grams);
    }
}

// 3. "tbs" is not a unit: 2 tablespoons of oil is logged as 2 grams.
#[test]
fn tbs_is_not_two_grams() {
    if let Ok(it) = nutrition::parse_line("2 tbs oil") {
        assert!((it.grams - 27.0).abs() < 5.0, "2 tbs oil became {} g", it.grams);
    }
}

// 4. A compound food resolves to a component: almond milk is milk, potato chips are potato.
#[test]
fn compound_food_is_not_its_component() {
    for piece in ["almond milk 200g", "potato chips 50g"] {
        if let Ok(it) = nutrition::parse_line(piece) {
            panic!("{piece:?} resolved to {} ({} g) instead of unknown", it.food.name, it.grams);
        }
    }
}

// 5. "1 head garlic" uses the clove weight (3 g) for a whole head.
#[test]
fn head_of_garlic_is_not_three_grams() {
    if let Ok(it) = nutrition::parse_line("1 head garlic") {
        assert!(it.grams >= 30.0, "1 head garlic became {} g", it.grams);
    }
}

// 6. A bare count above 12 of a piece food falls back to grams: 20 shrimp is 20 g, not 120 g.
#[test]
fn twenty_shrimp_are_pieces_not_grams() {
    if let Ok(it) = nutrition::parse_line("20 shrimp") {
        assert!((it.grams - 120.0).abs() < 1.0, "20 shrimp became {} g", it.grams);
    }
}

// 7. A weight glued to punctuation is not read: salmon(150g) is logged at the 100 g default.
#[test]
fn glued_weight_is_read_not_defaulted() {
    for piece in ["salmon(150g)", "salmon-150g"] {
        if let Ok(it) = nutrition::parse_line(piece) {
            assert!((it.grams - 150.0).abs() < 0.5, "{piece:?} became {} g (assumed {})", it.grams, it.assumed);
        }
    }
}

// 8. A non-meal sentence with a number and a food word becomes a meal: "Paid 12 dollars" -> 1800 g salmon.
#[test]
fn spending_sentence_is_not_a_meal() {
    let p = nutrition::parse_meal("Paid 12 dollars for salmon");
    assert!(p.items.is_empty(), "logged as meal: {:?}", p.items.iter().map(|i| (i.food.name, i.grams)).collect::<Vec<_>>());
}

// 11. "ml" is not a unit: 100 ml of honey is logged as 100 g with no warning.
#[test]
fn millilitres_are_not_grams() {
    assert!(nutrition::parse_line("honey 100ml").is_err(), "honey 100ml accepted as {:?} g", nutrition::parse_line("honey 100ml").map(|i| i.grams));
}

// 13. Two foods with no separator: only the longest alias is kept, the other is dropped without an unknown.
#[test]
fn second_food_in_one_phrase_is_not_dropped() {
    let p = nutrition::parse_meal("2 eggs with toast");
    let names: Vec<&str> = p.items.iter().map(|i| i.food.name).collect();
    assert!(names.contains(&"egg"), "egg dropped: items {:?} unknown {:?}", names, p.unknown);
}
