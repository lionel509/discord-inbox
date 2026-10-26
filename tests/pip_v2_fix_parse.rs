//! PR #114 round 1 fixes, parse side (F1): exact values for the grammar the breaker found wrong.
use discord_inbox::nutrition::{self, Item};

fn ok(piece: &str) -> Item {
    nutrition::parse_line(piece).unwrap_or_else(|u| panic!("{piece:?} unknown: {} ({})", u.text, u.hint))
}

fn is_err(piece: &str) -> bool {
    nutrition::parse_line(piece).is_err()
}

#[test]
fn one_pound_of_salmon_is_453_6_g() {
    let it = ok("1 lb salmon");
    assert_eq!(it.food.name, "salmon");
    assert!((it.grams - 453.6).abs() < 0.1, "got {}", it.grams);
}

#[test]
fn glued_weight_in_parens_or_after_dash_is_read() {
    for piece in ["salmon(150g)", "salmon-150g"] {
        let it = ok(piece);
        assert!((it.grams - 150.0).abs() < 0.01, "{piece:?} got {}", it.grams);
        assert!(!it.assumed, "{piece:?} was assumed");
    }
}

#[test]
fn typed_range_is_an_error_not_a_midpoint() {
    assert!(is_err("salmon 100-150g"));
}

#[test]
fn ten_kilo_cap_rejects_absurd_grams() {
    assert!(is_err("salmon 99999g"));
}

#[test]
fn twenty_shrimp_is_an_error_asking_pieces_or_grams() {
    assert!(is_err("20 shrimp"));
}

#[test]
fn shrimp_with_number_after_is_grams() {
    let it = ok("shrimp 20");
    assert_eq!(it.food.name, "shrimp");
    assert!((it.grams - 20.0).abs() < 0.01, "got {}", it.grams);
}

#[test]
fn banana_bread_is_not_banana() {
    assert!(is_err("banana bread 80g"));
}

#[test]
fn peanut_butter_is_its_own_food() {
    let it = ok("peanut butter 30g");
    assert_eq!(it.food.name, "peanut butter");
    assert!((it.grams - 30.0).abs() < 0.01);
}

#[test]
fn with_splits_a_second_food_and_keeps_it_as_unknown() {
    let p = nutrition::parse_meal("salmon 150g with rice");
    assert_eq!(p.items.len(), 1, "items {:?}", p.items.iter().map(|i| i.food.name).collect::<Vec<_>>());
    assert_eq!(p.items[0].food.name, "salmon");
    assert!((p.items[0].grams - 150.0).abs() < 0.01);
    assert_eq!(p.unknown.len(), 1, "unknown {:?}", p.unknown);
}

#[test]
fn looks_like_food_skips_clock_times_and_non_food_numbers() {
    for text in ["see you at 12:30", "slept 8 hours", "room 101", "ok 2"] {
        assert!(!nutrition::looks_like_food(text), "{text:?} read as food");
    }
}

#[test]
fn looks_like_food_keeps_real_meals() {
    for text in ["samon 100g", "½ cup", "chicken"] {
        assert!(nutrition::looks_like_food(text), "{text:?} not read as food");
    }
}
