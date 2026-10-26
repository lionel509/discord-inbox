//! U1: the local nutrition table and line parser. No model or network calls; numbers come from the table.
use discord_inbox::nutrition::{self, FOODS, Unit};
use std::collections::HashSet;

const BRIEF: &str = "100g salmon, 1/2 cup dry rice, cabbage + 1 tbsp oil";

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn brief_meal_parses_to_four_items_with_no_unknowns() {
    let p = nutrition::parse_meal(BRIEF);
    assert_eq!(p.items.len(), 4, "items: {:?}", p.items.iter().map(|i| i.food.name).collect::<Vec<_>>());
    assert!(p.unknown.is_empty(), "unexpected unknowns: {:?}", p.unknown);
    assert!(nutrition::has_veg(&p.items), "cabbage is a vegetable");
    let kcal = nutrition::macros(&p.items).kcal;
    assert!((600.0..=760.0).contains(&kcal), "expected about 690 kcal, got {kcal}");
}

#[test]
fn fraction_spellings_are_the_same_amount() {
    let a = nutrition::parse_meal("1/2 cup dry rice").items[0].grams;
    let b = nutrition::parse_meal("½ cup dry rice").items[0].grams;
    let c = nutrition::parse_meal("0.5 cup dry rice").items[0].grams;
    assert!(close(a, b) && close(a, c), "{a} {b} {c}");
    assert!(close(a, 92.5), "half a cup of dry rice is 92.5 g, got {a}");
    let cooked = nutrition::parse_meal("1/2 cup cooked rice").items[0].grams;
    assert!(!close(a, cooked), "dry and cooked rice must differ");
}

#[test]
fn bare_rice_is_unknown_with_a_dry_or_cooked_hint() {
    let p = nutrition::parse_meal("rice");
    assert!(p.items.is_empty());
    assert_eq!(p.unknown.len(), 1);
    assert!(p.unknown[0].hint.contains("dry rice") && p.unknown[0].hint.contains("cooked rice"), "{:?}", p.unknown[0]);
}

#[test]
fn unknown_food_is_reported_and_counts_nothing() {
    let p = nutrition::parse_meal("100g glaze");
    assert!(p.items.is_empty(), "a guessed food must never be counted");
    assert_eq!(p.unknown.len(), 1);
    assert_eq!(nutrition::macros(&p.items).kcal, 0.0);
}

#[test]
fn ounces_convert_at_28_35_grams() {
    let g = nutrition::parse_meal("3 oz salmon").items[0].grams;
    assert!(close(g, 85.05), "got {g}");
}

#[test]
fn tablespoon_of_oil_is_13_6_grams() {
    let it = &nutrition::parse_meal("1 tbsp oil").items[0];
    assert!(close(it.grams, 13.6), "got {}", it.grams);
    assert_eq!(it.unit, Unit::Tbsp);
}

#[test]
fn teaspoon_is_a_forty_eighth_of_a_cup() {
    let oil = nutrition::lookup("oil").unwrap();
    let want = oil.cup_g.unwrap() / 48.0;
    let g = nutrition::parse_meal("1 tsp oil").items[0].grams;
    assert!(close(g, want), "want {want}, got {g}");
}

#[test]
fn cup_of_a_food_without_a_cup_weight_is_unknown() {
    let p = nutrition::parse_meal("1 cup salmon");
    assert!(p.items.is_empty());
    assert_eq!(p.unknown.len(), 1);
    assert!(p.unknown[0].hint.contains("grams"), "{:?}", p.unknown[0]);
}

#[test]
fn bare_food_name_is_one_portion_and_marked_assumed() {
    let it = &nutrition::parse_meal("cabbage").items[0];
    assert!(close(it.grams, 100.0), "got {}", it.grams);
    assert!(it.assumed);
}

#[test]
fn merge_lets_the_typed_salmon_override_the_recipe_and_reports_it() {
    let typed = nutrition::parse_meal("150g salmon");
    let recipe = nutrition::parse_meal("100g salmon, 50g soy sauce");
    let (merged, overridden) = nutrition::merge(typed, recipe);
    let salmon: Vec<_> = merged.items.iter().filter(|i| i.food.name == "salmon").collect();
    assert_eq!(salmon.len(), 1, "salmon counted once");
    assert!(close(salmon[0].grams, 150.0), "typed weight wins, got {}", salmon[0].grams);
    assert!(merged.items.iter().any(|i| i.food.name == "soy sauce"), "untyped recipe lines are kept");
    assert_eq!(overridden, vec!["salmon".to_string()]);
}

#[test]
fn longest_alias_wins_so_rice_vinegar_is_not_rice() {
    assert_eq!(nutrition::lookup("rice vinegar").unwrap().name, "rice vinegar");
    assert_eq!(nutrition::lookup("dry rice").unwrap().name, "dry rice");
}

#[test]
fn table_has_the_fixed_demo_rows() {
    assert!(!FOODS.is_empty(), "table must not be empty");
    for name in ["salmon", "dry rice", "cooked rice", "cabbage", "oil"] {
        assert!(nutrition::lookup(name).is_ok(), "missing row {name}");
    }
}

#[test]
fn atwater_energy_matches_kcal_within_fifteen_percent() {
    assert!(!FOODS.is_empty(), "table must not be empty");
    for f in FOODS {
        if f.kcal >= 30.0 {
            let atwater = 4.0 * f.protein + 4.0 * f.carbs + 9.0 * f.fat;
            assert!((f.kcal - atwater).abs() <= 0.15 * f.kcal, "{}: kcal {} vs Atwater {}", f.name, f.kcal, atwater);
        }
    }
}

#[test]
fn every_alias_is_unique_across_rows() {
    assert!(!FOODS.is_empty(), "table must not be empty");
    let mut seen = HashSet::new();
    for f in FOODS {
        for a in f.aliases {
            assert!(seen.insert(*a), "alias {a} appears in two rows");
        }
    }
}

#[test]
fn every_vegetable_row_has_a_real_portion() {
    assert!(FOODS.iter().any(|f| f.veg), "table must contain vegetables");
    for f in FOODS.iter().filter(|f| f.veg) {
        assert!(f.portion_g >= 30.0, "{} portion {} g", f.name, f.portion_g);
    }
}

#[test]
fn uppercase_and_keeps_both_items() {
    let p = nutrition::parse_meal("100g salmon AND 1/2 cup dry rice");
    let names: Vec<&str> = p.items.iter().map(|i| i.food.name).collect();
    assert_eq!(names.len(), 2, "expected salmon and dry rice, got items {names:?} unknown {:?}", p.unknown);
    assert!(names.contains(&"salmon"), "salmon missing, items {names:?}");
}
