//! Breaker A (adversarial): nutrition line parser and recipe notes at cfadd1c.
//! Each test states the expected result. Failing tests are findings.
use discord_inbox::nutrition;
use discord_inbox::recipes;
use std::fs;
use std::path::PathBuf;

fn miso() -> recipes::Recipe {
    recipes::parse_note("Miso Salmon", "# Miso Salmon\n\n## Ingredients\n\n- 100 g salmon\n").expect("recipe loads")
}

// 1. A bare count of a food with a piece weight is read as grams.
#[test]
fn bare_count_banana_is_piece_not_grams() {
    let it = nutrition::parse_line("1 banana").expect("banana is in the table");
    assert!((it.grams - 118.0).abs() < 1.0, "expected about 118 g (one banana), got {} g ({:?})", it.grams, it.unit);
}

// 1b. A fraction of a piece food is read as grams too.
#[test]
fn half_banana_is_half_a_piece() {
    let it = nutrition::parse_line("1/2 banana").expect("banana is in the table");
    assert!((it.grams - 59.0).abs() < 1.0, "expected about 59 g, got {} g", it.grams);
}

// 1c. Recipe table cell "2 large" for egg becomes the line "2 large egg", which is 2 g.
#[test]
fn recipe_table_egg_with_size_word_is_not_2_grams() {
    let r = recipes::parse_note("Omelette", "# Omelette\n\n| Ingredient | Recipe |\n|---|---|\n| Egg | 2 large |\n").expect("loads");
    let pieces = recipes::ingredient_pieces(&recipes::resolve("1 serving omelette", std::slice::from_ref(&r)).expect("hit"));
    let item = nutrition::parse_line(&pieces[0]).expect("egg parses");
    assert!((item.grams - 100.0).abs() < 1.0, "expected two eggs (about 100 g), got {} g from line {:?}", item.grams, pieces[0]);
}

// 2. A vulgar fraction right after a whole number is dropped.
#[test]
fn unicode_fraction_after_whole_number() {
    let a = nutrition::parse_line("1½ cup dry rice").expect("rice parses");
    assert!((a.grams - 277.5).abs() < 0.5, "1½ cup dry rice: expected 277.5 g, got {} g", a.grams);
    let b = nutrition::parse_line("1 ½ cup dry rice").expect("rice parses");
    assert!((b.grams - 277.5).abs() < 0.5, "1 ½ cup dry rice: expected 277.5 g, got {} g", b.grams);
}

// 3. Uppercase AND does not split; salmon is lost and the whole phrase is read as dry rice.
#[test]
fn uppercase_and_keeps_both_items() {
    let p = nutrition::parse_meal("100g salmon AND 1/2 cup dry rice");
    let names: Vec<&str> = p.items.iter().map(|i| i.food.name).collect();
    assert_eq!(names.len(), 2, "expected salmon and dry rice, got items {names:?} unknown {:?}", p.unknown);
    assert!(names.contains(&"salmon"), "salmon missing, items {names:?}");
}

// 4a. A unit fraction the parser does not know is silently replaced by the default portion.
#[test]
fn eighth_fraction_is_not_a_default_portion() {
    let it = nutrition::parse_line("⅛ cup dry rice").expect("rice parses");
    assert!(!it.assumed, "⅛ cup dry rice was read as the default portion ({} g), quantity ignored", it.grams);
    assert!((it.grams - 23.1).abs() < 0.5, "expected about 23 g, got {} g", it.grams);
}

// 4b. A malformed number silently becomes the default portion.
#[test]
fn malformed_decimal_is_not_a_default_portion() {
    let it = nutrition::parse_line("1..5 cup dry rice");
    match it {
        Err(_) => {}
        Ok(i) => assert!(!i.assumed, "1..5 cup dry rice was read as the default portion ({} g)", i.grams),
    }
}

// 5. A quantity typed after the food is ignored; the default portion is used.
#[test]
fn quantity_after_food_name_is_used() {
    let it = nutrition::parse_line("salmon 200 g").expect("salmon parses");
    assert!((it.grams - 200.0).abs() < 0.5, "salmon 200 g: expected 200 g, got {} g (assumed={})", it.grams, it.assumed);
}

// 6. A thousands separator splits the line: "1" is unknown and "000 g dry rice" is 0 g.
#[test]
fn thousands_separator_is_one_quantity() {
    let p = nutrition::parse_meal("1,000 g dry rice");
    assert!(p.unknown.is_empty(), "unexpected unknowns: {:?}", p.unknown);
    assert_eq!(p.items.len(), 1, "expected one rice item, got {:?}", p.items.iter().map(|i| i.grams).collect::<Vec<_>>());
    assert!((p.items[0].grams - 1000.0).abs() < 0.5, "1,000 g dry rice: expected 1000 g, got {} g", p.items[0].grams);
}

// 7. A range "1-2 cups" is read as 1 unit of the thing.
#[test]
fn range_quantity_is_not_the_low_end_alone() {
    let it = nutrition::parse_line("1-2 cups dry rice").expect("rice parses");
    assert!(it.grams >= 185.0, "1-2 cups dry rice: expected at least 185 g, got {} g", it.grams);
}

// 7b. Same range in a recipe table cell: the recipe line becomes "1 g".
#[test]
fn recipe_table_range_is_not_one_gram() {
    let r = recipes::parse_note("Rice Bowl", "# Rice Bowl\n\n| Ingredient | Recipe |\n|---|---|\n| Dry rice | 1–2 cups |\n").expect("loads");
    let pieces = recipes::ingredient_pieces(&recipes::resolve("1 serving rice bowl", std::slice::from_ref(&r)).expect("hit"));
    let item = nutrition::parse_line(&pieces[0]).expect("parses");
    assert!(item.grams >= 185.0, "table cell 1–2 cups: expected at least 185 g, got {} g from line {:?}", item.grams, pieces[0]);
}

// 8. A leading minus is silently dropped: "-100 g salmon" becomes 100 g.
#[test]
fn negative_quantity_is_rejected() {
    let p = nutrition::parse_meal("-100 g salmon");
    let grams: Vec<f64> = p.items.iter().map(|i| i.grams).collect();
    assert!(p.items.is_empty() && !p.unknown.is_empty(), "-100 g salmon was accepted as {grams:?} g");
}

// 9. A 400-digit quantity parses to infinity and the kcal saturates.
#[test]
fn absurd_quantity_is_rejected_or_finite() {
    let line = format!("{} g salmon", "9".repeat(400));
    match nutrition::parse_line(&line) {
        Err(_) => {}
        Ok(it) => {
            let shown = nutrition::item_line(&it);
            assert!(it.grams.is_finite(), "absurd quantity became grams={}; item line: {shown}", it.grams);
        }
    }
}

// 10. Spaced multiplier "2 x" before the recipe name is ignored.
#[test]
fn spaced_x_multiplier_before_name() {
    let r = miso();
    let hit = recipes::resolve("2 x miso salmon", std::slice::from_ref(&r)).expect("hit");
    assert_eq!(hit.servings, 2.0, "2 x miso salmon: expected 2 servings, got {}", hit.servings);
}

// 11. "2 servings of <recipe>" falls back to one serving.
#[test]
fn servings_of_phrase() {
    let r = miso();
    let hit = recipes::resolve("2 servings of miso salmon", std::slice::from_ref(&r)).expect("hit");
    assert_eq!(hit.servings, 2.0, "2 servings of miso salmon: expected 2, got {}", hit.servings);
}

// 12. A mixed-number serving count loses its whole part.
#[test]
fn mixed_number_servings() {
    let r = miso();
    let hit = recipes::resolve("1 1/2 servings miso salmon", std::slice::from_ref(&r)).expect("hit");
    assert_eq!(hit.servings, 1.5, "1 1/2 servings miso salmon: expected 1.5, got {}", hit.servings);
}

// 13. "0 servings" silently becomes one full serving.
#[test]
fn zero_servings_is_not_a_full_serving() {
    let r = miso();
    if let Some(hit) = recipes::resolve("0 servings miso salmon", std::slice::from_ref(&r)) {
        assert_eq!(hit.servings, 0.0, "0 servings miso salmon: expected 0 servings, got {}", hit.servings);
    }
}

// 14. A recipe file with an uppercase .MD extension is never loaded.
#[test]
fn uppercase_md_extension_is_loaded() {
    let dir: PathBuf = std::env::temp_dir().join(format!("pip-break-a-{}-md", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Miso Salmon.MD"), "# Miso Salmon\n\n## Ingredients\n\n- 100 g salmon\n").unwrap();
    let loaded = recipes::load(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(loaded.len(), 1, "Miso Salmon.MD was not loaded; recipes found: {}", loaded.len());
}

// 15. A table outside the ingredients section is read as ingredient lines.
#[test]
fn method_table_is_not_ingredients() {
    let text = "# Miso Salmon\n\n## Method\n\n| Step | Time |\n|---|---|\n| Sear | 5 min |\n\n## Ingredients\n\n- 100 g salmon\n";
    let r = recipes::parse_note("Miso Salmon", text).expect("loads");
    assert_eq!(r.lines, vec!["100 g salmon".to_string()], "non-ingredient table leaked into lines: {:?}", r.lines);
}
