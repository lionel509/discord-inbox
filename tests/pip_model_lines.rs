//! #130 unit B: a model-read item is marked `(estimated)` unless his typed grams say otherwise; table foods match exactly.
use discord_inbox::nutrition::{self, Food, Item, Unit};

fn model_item(grams: f64, estimated: bool) -> Item {
    Item { food: Food::model("cooked egg noodles", 138.0, 4.5, 25.0, 2.1, false), grams, unit: Unit::G, qty: grams, assumed: false, estimated, scale: false }
}

#[test]
fn parse_line_items_are_not_estimated() {
    let p = nutrition::parse_meal("salmon 150g");
    assert_eq!(p.items.len(), 1, "{:?}", p.unknown);
    assert!(!p.items[0].estimated);
}

#[test]
fn estimated_line_is_marked() {
    assert_eq!(nutrition::item_line(&model_item(100.0, true)), "100 g cooked egg noodles (estimated) · 138 kcal");
    assert_eq!(nutrition::item_line(&model_item(100.0, false)), "100 g cooked egg noodles · 138 kcal");
    let m = nutrition::macros(&[model_item(100.0, true)]);
    assert!((m.kcal - 138.0).abs() < 1e-9, "{m:?}");
}

#[test]
fn typed_grams_reads_mass_amounts() {
    let g = nutrition::typed_grams("100 grams of noodle 1 kg chicken 2 oz butter 3 cups rice");
    assert_eq!(g.len(), 3, "cups are a volume, not grams: {g:?}");
    assert!((g[0] - 100.0).abs() < 0.01, "{g:?}");
    assert!((g[1] - 1000.0).abs() < 0.01, "{g:?}");
    assert!((g[2] - 56.70).abs() < 0.05, "{g:?}");
}

#[test]
fn table_food_is_exact_only() {
    assert_eq!(nutrition::table_food("Egg ").map(|f| f.name), Some("egg"));
    assert!(nutrition::table_food("chicken fried rice").is_none(), "no fuzzy match for a model name");
    assert_eq!(nutrition::table_food("steamed rice").map(|f| f.name), Some("cooked rice"));
}
