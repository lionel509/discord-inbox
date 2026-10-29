//! #146: saved foods from label photos (`foods`): the note format, the upsert, and `take`, which turns a saved food
//! into items by pieces, grams or servings. Fixtures are made up ("Northwind"); scratch dirs live under the temp dir.
use discord_inbox::foods::{self, SavedFood};
use discord_inbox::nutrition::{self, Unit};
use discord_inbox::pip_model::Label;
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pip-foods-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn label(name: &str, fat: Option<f64>) -> Label {
    Label {
        name: name.to_string(),
        serving_g: Some(140.0),
        serving_pc: Some(8.0),
        kcal: Some(230.0),
        protein: Some(9.0),
        carbs: Some(28.0),
        fat,
    }
}

fn wontons(last: Option<(f64, Unit)>) -> SavedFood {
    SavedFood {
        name: "Northwind Pork Wontons".to_string(),
        names: vec!["Northwind Pork Wontons".to_string()],
        serving_g: Some(140.0),
        serving_pc: Some(8.0),
        kcal: Some(230.0),
        protein: Some(9.0),
        carbs: Some(28.0),
        fat: Some(9.0),
        last,
        saved: "2026-10-29".to_string(),
    }
}

#[test]
fn full_label_has_nothing_missing_and_a_missing_fat_is_listed() {
    let f = foods::from_label(&label("Northwind Pork Wontons", Some(9.0)), "2026-10-29").unwrap();
    assert!(foods::missing(&f).is_empty(), "{:?}", foods::missing(&f));
    let g = foods::from_label(&label("Northwind Pork Wontons", None), "2026-10-29").unwrap();
    assert_eq!(foods::missing(&g), vec!["fat"]);
}

#[test]
fn a_label_with_no_piece_count_is_not_missing_one() {
    let mut l = label("Northwind Salmon Fillets", Some(9.0));
    l.serving_pc = None;
    let f = foods::from_label(&l, "2026-10-29").unwrap();
    assert!(foods::missing(&f).is_empty());
}

#[test]
fn an_impossible_label_is_refused_and_the_name_is_made_safe_for_a_file() {
    let mut l = label("Northwind Pork Wontons", Some(9.0));
    l.kcal = Some(2500.0);
    assert!(foods::from_label(&l, "2026-10-29").is_err());
    let f = foods::from_label(&label("A/B: wontons?", Some(9.0)), "2026-10-29").unwrap();
    assert_eq!(f.name, "A B wontons");
}

#[test]
fn the_block_round_trips_including_the_last_count() {
    let f = wontons(Some((24.0, Unit::Piece)));
    let block = foods::format_block(&f);
    assert!(block.contains("- last: 24 pc"), "{block}");
    assert!(block.contains("- serving: 140 g · 8 pc"), "{block}");
    let back = foods::parse_block(&block).unwrap();
    assert_eq!(back.serving_g, f.serving_g);
    assert_eq!(back.serving_pc, f.serving_pc);
    assert_eq!(back.kcal, f.kcal);
    assert_eq!(back.last, Some((24.0, Unit::Piece)));
}

#[test]
fn upsert_then_upsert_again_keeps_one_note_and_one_index_line() {
    let dir = scratch("upsert");
    let path = foods::upsert(&dir, &wontons(None)).unwrap();
    assert!(path.ends_with("Northwind Pork Wontons.md"), "{path:?}");
    assert_eq!(foods::load(&dir).len(), 1);
    let mut again = wontons(None);
    again.kcal = Some(250.0);
    foods::upsert(&dir, &again).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("- per serving: 250 kcal"), "{text}");
    let mds = fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "md")).count();
    assert_eq!(mds, 2, "one food note and the index");
    let index = fs::read_to_string(dir.join("Foods.md")).unwrap();
    assert_eq!(index.lines().filter(|l| l.starts_with("- [[")).count(), 1, "{index}");
}

#[test]
fn a_hand_made_note_keeps_its_lines_and_gets_the_block_added() {
    let dir = scratch("handmade");
    let original = "---\ntags:\n  - making\n  - ingredient\n---\n\n# Northwind Pork Wontons\n\nMy own line.\n\n**Hub:** [[Home]]\n";
    fs::write(dir.join("Northwind Pork Wontons.md"), original).unwrap();
    foods::upsert(&dir, &wontons(None)).unwrap();
    let text = fs::read_to_string(dir.join("Northwind Pork Wontons.md")).unwrap();
    assert!(text.contains("My own line."), "{text}");
    assert!(text.contains("**Hub:** [[Home]]"), "{text}");
    assert!(text.contains("<!-- pip:food -->"), "{text}");
}

#[test]
fn a_written_note_carries_no_path_no_photo_and_no_folder_name() {
    let dir = scratch("privacy");
    let path = foods::upsert(&dir, &wontons(None)).unwrap();
    // The close marker is `<!-- /pip:food -->`; its slash is not a path.
    let text = fs::read_to_string(&path).unwrap().replace("<!-- /pip:food -->", "");
    for bad in [".jpg", ".png", "/", "Nutrition"] {
        assert!(!text.contains(bad), "{bad} in {text}");
    }
}

#[test]
fn pieces_of_a_label_food_are_counted_by_pieces() {
    let f = wontons(None);
    let t = foods::take("24 wontons", &[f], true);
    assert!(t.unknown.is_empty(), "{:?}", t.unknown);
    assert_eq!(t.rest, "");
    assert_eq!(t.items.len(), 1);
    let it = &t.items[0];
    assert_eq!(it.unit, Unit::Piece);
    assert_eq!(it.qty, 24.0);
    assert!((it.grams - 420.0).abs() < 1e-9, "{}", it.grams);
    let line = nutrition::item_line(it);
    assert_eq!(line, "420 g Northwind Pork Wontons (24 pc, label) · 690 kcal");
}

#[test]
fn a_bare_name_uses_the_last_count_he_logged_else_one_serving() {
    let t = foods::take("wontons", &[wontons(Some((24.0, Unit::Piece)))], true);
    assert_eq!(t.items.len(), 1);
    assert!(t.items[0].assumed);
    assert!(nutrition::item_line(&t.items[0]).ends_with("(default 24 pc, label) · 690 kcal"));
    let t = foods::take("wontons", &[wontons(None)], true);
    assert_eq!(t.items[0].grams, 140.0);
    assert!(nutrition::item_line(&t.items[0]).contains("(default 1 serving, label)"));
}

#[test]
fn grams_and_servings_scale_the_label() {
    let t = foods::take("wontons 200g", &[wontons(None)], true);
    assert_eq!(t.items[0].grams, 200.0);
    assert!(nutrition::item_line(&t.items[0]).contains("(200 g, label) · 329 kcal"));
    let t = foods::take("3 servings wontons", &[wontons(None)], true);
    assert_eq!(t.items[0].grams, 420.0);
    assert!(nutrition::item_line(&t.items[0]).contains("(3 servings, label)"));
}

#[test]
fn a_piece_count_with_no_piece_count_on_the_label_is_refused_not_grams() {
    let mut f = wontons(None);
    f.serving_pc = None;
    let t = foods::take("24 wontons", &[f], true);
    assert!(t.items.is_empty(), "a bare 24 must never become 24 g");
    assert_eq!(t.unknown.len(), 1);
    assert!(t.unknown[0].hint.contains("no piece count"), "{:?}", t.unknown);
}

#[test]
fn a_table_food_named_in_part_stays_on_the_table() {
    let f = SavedFood {
        name: "Northwind Salmon Fillets".to_string(),
        names: vec!["Northwind Salmon Fillets".to_string()],
        ..wontons(None)
    };
    let t = foods::take("salmon 150g", std::slice::from_ref(&f), true);
    assert!(t.items.is_empty());
    assert_eq!(t.rest, "salmon 150g");
    let t = foods::take("northwind salmon fillets 150g", &[f], true);
    assert_eq!(t.items.len(), 1);
}

#[test]
fn two_saved_foods_that_both_fit_are_refused_by_name() {
    let mut other = wontons(None);
    other.name = "Other Wontons".to_string();
    other.names = vec!["Other Wontons".to_string()];
    let t = foods::take("24 wontons", &[wontons(None), other], true);
    assert!(t.items.is_empty());
    assert!(t.unknown[0].hint.contains("more than one"), "{:?}", t.unknown);
}

#[test]
fn a_recipe_line_uses_the_label_and_a_bare_name_stays_out_of_it() {
    let t = foods::take("[[Northwind Pork Wontons]] 8 pieces", &[wontons(None)], false);
    assert_eq!(t.items.len(), 1);
    assert_eq!(t.items[0].grams, 140.0);
    assert_eq!(t.items[0].qty, 8.0);
    let t = foods::take("wontons", &[wontons(None)], false);
    assert!(t.items.is_empty());
    assert_eq!(t.rest, "wontons");
}

#[test]
fn a_saved_food_sits_beside_a_table_food_in_one_message() {
    let t = foods::take("cooked rice 180g, 24 wontons", &[wontons(None)], true);
    assert_eq!(t.items.len(), 1);
    assert_eq!(t.rest, "cooked rice 180g");
}

fn mac_with_cheese() -> SavedFood {
    SavedFood {
        name: "Northwind Mac with Cheese".to_string(),
        names: vec!["Northwind Mac with Cheese".to_string()],
        serving_g: Some(140.0),
        serving_pc: None,
        kcal: Some(300.0),
        protein: Some(10.0),
        carbs: Some(40.0),
        fat: Some(10.0),
        last: None,
        saved: "2026-10-29".to_string(),
    }
}

#[test]
fn a_saved_name_with_with_in_it_is_one_piece() {
    let t = foods::take("northwind mac with cheese 200g", &[mac_with_cheese()], true);
    assert_eq!(t.items.len(), 1, "{:?}", t.items.iter().map(|i| (i.food.name, i.grams)).collect::<Vec<_>>());
    assert!((t.items[0].grams - 200.0).abs() < 1e-9);
    assert_eq!(t.rest, "");
}

#[test]
fn two_long_product_names_that_share_sixty_characters_stay_two_foods() {
    let dir = scratch("long-names");
    let a = foods::from_label(&label("Northwind Family Size Chicken Teriyaki Flavoured Bites Party Pack Original", Some(9.0)), "2026-10-29").unwrap();
    let b = foods::from_label(&label("Northwind Family Size Chicken Teriyaki Flavoured Bites Party Pack Spicy", Some(9.0)), "2026-10-29").unwrap();
    foods::upsert(&dir, &a).unwrap();
    foods::upsert(&dir, &b).unwrap();
    let kept = foods::load(&dir);
    assert_eq!(kept.len(), 2, "{:?}", kept.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    assert!(kept.iter().any(|f| f.name == a.name) && kept.iter().any(|f| f.name == b.name));
}

#[test]
fn a_product_name_too_long_to_file_is_refused_not_cut() {
    let long = "Northwind ".repeat(30);
    let err = foods::from_label(&label(&long, Some(9.0)), "2026-10-29").unwrap_err();
    assert!(err.contains("too long"), "{err}");
}
