//! The receipt food rows: fried chicken, mashed potato, gravy, regular and diet soda.
use discord_inbox::nutrition::{looks_like_food, parse_meal};

#[test]
fn receipt_lines_resolve_to_the_new_rows() {
    let p = parse_meal("2 pc fried chicken, mashed potato with cajun gravy");
    assert!(p.unknown.is_empty(), "unknowns: {:?}", p.unknown);
    let names: Vec<(&str, f64, bool)> = p.items.iter().map(|i| (i.food.name, i.grams, i.assumed)).collect();
    assert_eq!(
        names,
        vec![("fried chicken", 280.0, false), ("mashed potato", 150.0, true), ("gravy", 60.0, true)]
    );
}

#[test]
fn regular_soda_is_within_two_percent_of_a_twelve_ounce_can() {
    let p = parse_meal("regular soda 12 fl oz");
    assert!(p.unknown.is_empty(), "unknowns: {:?}", p.unknown);
    let g = p.items[0].grams;
    assert_eq!(p.items[0].food.name, "regular soda");
    assert!((g - 369.0).abs() / 369.0 < 0.02, "grams {g}");
}

#[test]
fn diet_sodas_resolve_to_zero_kcal() {
    for line in ["diet coke", "coke zero"] {
        let p = parse_meal(line);
        assert!(p.unknown.is_empty(), "{line}: unknowns {:?}", p.unknown);
        assert_eq!(p.items[0].food.name, "diet soda", "{line}");
        assert_eq!(p.items[0].food.kcal, 0.0, "{line}");
    }
}

#[test]
fn bare_soda_is_still_unknown_and_baking_soda_is_not_food() {
    let p = parse_meal("soda");
    assert!(p.items.is_empty());
    assert_eq!(p.unknown.len(), 1);
    assert!(!looks_like_food("out of baking soda"));
}

#[test]
fn existing_matches_keep_their_rows() {
    assert_eq!(parse_meal("chicken").unknown.len(), 1, "chicken stays ambiguous");
    assert_eq!(parse_meal("potato 150g").items[0].food.name, "potato");
    assert_eq!(parse_meal("mashed potatoes 150g").items[0].food.name, "mashed potato");
    assert_eq!(parse_meal("fried chicken thigh 140g").items[0].food.name, "fried chicken");
    assert_eq!(parse_meal("chicken breast 150g").items[0].food.name, "chicken breast");
    assert_eq!(parse_meal("chicken thigh 150g").items[0].food.name, "chicken thigh");
}
