//! #159 U2: the reader's dish and product picks, the source tails, and the prompt's candidate lists. Stubbed replies only.
use discord_inbox::dishes::Dish;
use discord_inbox::nutrition::{self, Parsed};
use discord_inbox::pantry::Product;
use discord_inbox::pip_model::{self, Sources};
use std::path::PathBuf;

fn product() -> Product {
    Product { name: "Farmhouse Sourdough Bread".into(), brand: "Pepperidge Farm".into(), size: "24 oz".into(), bought: "2026-11-08".into() }
}

fn dish() -> Dish {
    Dish { note: "Garlic Spinach Rice".into(), lines: vec!["About 520 kcal a serving.".into()] }
}

fn sources(products: Vec<Product>, dishes: Vec<Dish>) -> Sources {
    Sources { foods: Vec::new(), dishes, products }
}

fn reply(items: &str) -> String {
    format!("{{\"items\":[{items}],\"unreadable\":[]}}")
}

fn obj(name: &str, grams: f64, kcal_100: f64, extra: &str) -> String {
    format!("{{\"name\":\"{name}\",\"grams\":{grams},\"typed\":false,\"kcal_100\":{kcal_100},\"protein_100\":9,\"carbs_100\":49,\"fat_100\":3,\"veg\":false{extra}}}")
}

fn parse(text: &str, out: &str, src: &Sources) -> Parsed {
    pip_model::parse_sources(text, out, src).expect("the reply parses")
}

fn kcal(item: &nutrition::Item) -> f64 {
    item.food.kcal * item.grams / 100.0
}

#[test]
fn empty_sources_prompt_is_prompt_saved() {
    let photos: Vec<PathBuf> = Vec::new();
    assert_eq!(pip_model::prompt_sources("2 eggs", &photos, &Sources::default()), pip_model::prompt_saved("2 eggs", &photos, &[]));
}

#[test]
fn product_pick_names_its_source() {
    let out = reply(&obj("sourdough bread", 43.0, 265.0, ",\"product\":\"Farmhouse Sourdough Bread\""));
    let p = parse("1 slice sourdough", &out, &sources(vec![product()], Vec::new()));
    let item = &p.items[0];
    assert_eq!(item.grams, 43.0);
    let tail = nutrition::source_tail(item).expect("a bought product names its source");
    assert!(tail.ends_with("bought: Farmhouse Sourdough Bread)"), "{tail}");
    assert!(nutrition::item_line(item).contains("bought: Farmhouse Sourdough Bread)"));
}

#[test]
fn product_values_beat_the_table() {
    // The table's egg is 143 kcal per 100 g; the product's own label says 150.
    let out = reply(&obj("egg", 50.0, 150.0, ",\"product\":\"Organic Eggs\""));
    let src = sources(vec![Product { name: "Organic Eggs".into(), brand: "Costco".into(), size: "-".into(), bought: "2026-10-31".into() }], Vec::new());
    let p = parse("2 eggs", &out, &src);
    assert!((kcal(&p.items[0]) - 75.0).abs() < 0.5, "product label kcal, not the table's: {}", kcal(&p.items[0]));
}

#[test]
fn dish_pick_uses_the_notes_kcal() {
    let out = reply(&obj("garlic spinach rice", 300.0, 173.0, ",\"dish\":\"Garlic Spinach Rice\",\"dish_kcal\":520"));
    let p = parse("garlic spinach rice", &out, &sources(Vec::new(), vec![dish()]));
    let item = &p.items[0];
    assert!((kcal(item) - 520.0).abs() < 0.5, "{}", kcal(item));
    assert_eq!(nutrition::source_tail(item).as_deref(), Some(" (1 serving, recipe: Garlic Spinach Rice)"));
}

#[test]
fn dish_kcal_not_in_note_is_refused() {
    let out = reply(&obj("garlic spinach rice", 300.0, 173.0, ",\"dish\":\"Garlic Spinach Rice\",\"dish_kcal\":900"));
    let p = parse("garlic spinach rice", &out, &sources(Vec::new(), vec![dish()]));
    assert!(p.unknown.iter().any(|u| u.hint.contains("not a number in that note")), "{:?}", p.unknown);
    assert!(nutrition::source_tail(&p.items[0]).is_none(), "a refused kcal is not a recipe source");
}

#[test]
fn marks_ignored_when_lists_empty() {
    let out = reply(&obj("sourdough bread", 43.0, 265.0, ",\"product\":\"Farmhouse Sourdough Bread\",\"dish\":\"Garlic Spinach Rice\",\"dish_kcal\":520"));
    let p = parse("1 slice sourdough", &out, &Sources::default());
    assert_eq!(p.items.len(), 1);
    assert!(nutrition::source_tail(&p.items[0]).is_none());
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn existing_lines_unchanged() {
    // Replies with no marks must read exactly as the saved-foods reader reads them today.
    for out in [
        reply(&obj("egg", 100.0, 143.0, "")),
        reply(&obj("rice", 150.0, 130.0, "")),
    ] {
        for text in ["2 eggs", "150 g rice"] {
            let new = parse(text, &out, &sources(vec![product()], vec![dish()]));
            let old = pip_model::parse_saved(text, &out, &[]).expect("the old reader parses");
            let (a, b): (Vec<String>, Vec<String>) = (new.items.iter().map(nutrition::item_line).collect(), old.items.iter().map(nutrition::item_line).collect());
            assert_eq!(a, b, "{text}");
        }
    }
}

#[test]
fn typed_weight_wins_over_product_and_dish() {
    for g in [1u32, 7, 150, 999, 2500, 5000] {
        let out = reply(&obj("honey ham", 999.0, 110.0, ",\"product\":\"Uncured Black Forest Ham\",\"dish\":\"Garlic Spinach Rice\",\"dish_kcal\":520"));
        let p = parse(&format!("{g}g honey ham"), &out, &sources(vec![product()], vec![dish()]));
        assert_eq!(p.items[0].grams, g as f64, "a typed weight is authoritative (#130)");
    }
}

#[test]
fn only_listed_names_become_sources() {
    let out = reply(&obj("mystery", 80.0, 200.0, ",\"product\":\"Made Up Thing\""));
    let p = parse("some mystery", &out, &sources(vec![product()], Vec::new()));
    assert!(p.unknown.iter().any(|u| u.hint.contains("is not in your pantry")), "{:?}", p.unknown);
    assert!(nutrition::source_tail(&p.items[0]).is_none(), "an unlisted name is never a source");
}

#[test]
fn source_order() {
    // Dish, then product, then a plain generic item: each line names only the source it used.
    let out = reply(&[
        obj("garlic spinach rice", 300.0, 173.0, ",\"dish\":\"Garlic Spinach Rice\",\"dish_kcal\":520"),
        obj("sourdough bread", 43.0, 265.0, ",\"product\":\"Farmhouse Sourdough Bread\""),
        obj("apple", 150.0, 52.0, ""),
    ]
    .join(","));
    let p = parse("garlic spinach rice, sourdough, apple", &out, &sources(vec![product()], vec![dish()]));
    assert_eq!(p.items.len(), 3);
    assert!(nutrition::source_tail(&p.items[0]).unwrap().contains("recipe: Garlic Spinach Rice"));
    assert!(nutrition::source_tail(&p.items[1]).unwrap().contains("bought: Farmhouse Sourdough Bread"));
    assert!(nutrition::source_tail(&p.items[2]).is_none());
}
