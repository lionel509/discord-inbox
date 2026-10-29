//! #146 round 7: the reader marks an item as one of his saved foods (`"saved"`, `"count"`), and `parse_saved` prices the
//! mark from the label (`foods::price`). Stubbed replies only; no live model.
use discord_inbox::foods::{self, SavedFood};
use discord_inbox::nutrition::{self, Unit};
use discord_inbox::pip_model;
use std::path::PathBuf;

fn w(last: Option<(f64, Unit)>) -> SavedFood {
    SavedFood {
        name: "Northwind Pork Wontons".to_string(),
        names: vec!["Northwind Pork Wontons".to_string(), "Pork Wontons".to_string(), "Wontons".to_string()],
        serving_g: Some(140.0),
        serving_pc: Some(8.0),
        kcal: Some(230.0),
        protein: Some(0.0),
        carbs: Some(0.0),
        fat: Some(0.0),
        last,
        saved: "2026-10-29".to_string(),
    }
}

fn b() -> SavedFood {
    SavedFood {
        name: "Northwind Rice Bowl".to_string(),
        names: vec!["Northwind Rice Bowl".to_string(), "Rice Bowl".to_string()],
        serving_g: Some(250.0),
        serving_pc: None,
        kcal: Some(400.0),
        protein: Some(0.0),
        carbs: Some(0.0),
        fat: Some(0.0),
        last: None,
        saved: "2026-10-29".to_string(),
    }
}

fn reply(items: &[String]) -> String {
    format!("{{\"items\":[{}],\"unreadable\":[]}}", items.join(","))
}

fn item(name: &str, grams: f64, kcal: f64, source: &str, saved: Option<&str>, count: Option<f64>) -> String {
    let mut s = format!(r#"{{"name":"{name}","grams":{grams},"source":"{source}","kcal_100":{kcal},"protein_100":7,"carbs_100":20,"fat_100":6,"veg":false"#);
    if let Some(m) = saved {
        s.push_str(&format!(r#","saved":"{m}""#));
    }
    if let Some(c) = count {
        s.push_str(&format!(r#","count":{c}"#));
    }
    s.push('}');
    s
}

fn marked(name: &str, mark: &str, count: Option<f64>) -> String {
    reply(&[item(name, 420.0, 164.0, "estimate", Some(mark), count)])
}

#[test]
fn a_valid_mark_with_his_count_is_priced_from_the_label() {
    let p = pip_model::parse_saved("24 wontons", &marked("wontons", "Northwind Pork Wontons", Some(24.0)), &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    let i = &p.items[0];
    assert_eq!(i.food.source, "label");
    assert_eq!(i.grams, 420.0);
    assert_eq!(i.unit, Unit::Piece);
    assert_eq!(i.qty, 24.0);
    assert!(!i.estimated && !i.assumed);
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
    assert_eq!(nutrition::item_line(i), "420 g Northwind Pork Wontons (24 pc, label) \u{b7} 690 kcal");
}

#[test]
fn a_count_he_did_not_type_falls_back_to_his_default() {
    let r = marked("wontons", "Northwind Pork Wontons", Some(8.0));
    let p = pip_model::parse_saved("24 dumplings", &r, &[w(Some((12.0, Unit::Piece)))]).unwrap();
    assert_eq!(p.items.len(), 1);
    let i = &p.items[0];
    assert!(i.assumed);
    assert_eq!((i.qty, i.unit), (12.0, Unit::Piece));
    assert!(p.unknown.is_empty());
    let p = pip_model::parse_saved("24 dumplings", &r, &[w(None)]).unwrap();
    let i = &p.items[0];
    assert!(i.assumed);
    assert_eq!((i.grams, i.unit, i.qty), (140.0, Unit::Portion, 1.0));
    assert!(p.unknown.is_empty());
}

#[test]
fn a_typed_weight_beats_the_count() {
    let r = marked("wontons", "Northwind Pork Wontons", Some(24.0));
    let p = pip_model::parse_saved("wontons 200g", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    let i = &p.items[0];
    assert_eq!((i.grams, i.unit), (200.0, Unit::G));
    assert!(!i.assumed);
    assert!(nutrition::item_line(i).contains("(200 g, label)"), "{}", nutrition::item_line(i));
}

#[test]
fn a_scale_reading_of_a_saved_food_uses_the_label_per_gram() {
    let r = reply(&[item("wontons", 300.0, 164.0, "scale", Some("Northwind Pork Wontons"), None)]);
    let p = pip_model::parse_saved("", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    let i = &p.items[0];
    assert_eq!(i.grams, 300.0);
    assert!(i.scale && !i.estimated);
    assert_eq!(i.food.source, "label");
    assert!(nutrition::item_line(i).ends_with("(scale) \u{b7} 493 kcal"), "{}", nutrition::item_line(i));
}

#[test]
fn a_count_on_a_label_with_no_piece_count_keeps_the_estimate_and_explains() {
    let r = reply(&[item("rice bowl", 300.0, 150.0, "estimate", Some("Northwind Rice Bowl"), Some(3.0))]);
    let p = pip_model::parse_saved("3 rice bowls", &r, &[b()]).unwrap();
    assert_eq!(p.items.len(), 1);
    let i = &p.items[0];
    assert!(i.estimated);
    assert_eq!(i.food.name, "rice bowl");
    assert_eq!(i.grams, 300.0);
    assert_eq!(p.unknown.len(), 1, "{:?}", p.unknown);
    let u = &p.unknown[0];
    assert_eq!(u.text, "saved food \"Northwind Rice Bowl\"");
    assert!(u.hint.contains("no piece count"), "{}", u.hint);
    assert!(u.hint.contains("northwind rice bowl 200g"), "{}", u.hint);
    assert!(u.hint.contains("rice bowl is counted as the reader's estimate"), "{}", u.hint);
}

#[test]
fn a_mark_for_a_note_missing_kcal_keeps_the_estimate() {
    let mut f = w(None);
    f.kcal = None;
    let p = pip_model::parse_saved("24 wontons", &marked("wontons", "Northwind Pork Wontons", Some(24.0)), &[f]).unwrap();
    assert_eq!(p.items.len(), 1);
    assert!(p.items[0].estimated);
    assert_eq!(p.unknown.len(), 1);
    assert!(p.unknown[0].hint.contains("missing kcal"), "{}", p.unknown[0].hint);
}

#[test]
fn a_mark_by_alias_or_case_is_accepted() {
    for mark in ["pork wontons", "WONTONS", "northwind-pork-wontons"] {
        let p = pip_model::parse_saved("24 wontons", &marked("wontons", mark, Some(24.0)), &[w(None)]).unwrap();
        assert_eq!(p.items.len(), 1, "{mark}");
        assert_eq!(p.items[0].food.source, "label", "{mark}");
        assert_eq!(p.items[0].grams, 420.0, "{mark}");
        assert!(p.unknown.is_empty(), "{mark}: {:?}", p.unknown);
    }
}

#[test]
fn an_ambiguous_mark_keeps_the_estimate() {
    let mut other = w(None);
    other.name = "Acme Wontons".to_string();
    other.names = vec!["Acme Wontons".to_string(), "Wontons".to_string()];
    let p = pip_model::parse_saved("24 wontons", &marked("wontons", "Wontons", Some(24.0)), &[w(None), other]).unwrap();
    assert_eq!(p.items.len(), 1);
    assert!(p.items[0].estimated);
    // Round 8: each typed saved food that no mark priced is also named under Not counted.
    assert_eq!(p.unknown.len(), 3, "{:?}", p.unknown);
    assert!(p.unknown[0].hint.contains("more than one saved food"), "{}", p.unknown[0].hint);
    for u in &p.unknown[1..] {
        assert!(u.hint.contains("if a line above is this food, that is the reader's estimate"), "{}", u.hint);
    }
    assert!(p.unknown.iter().all(|u| !u.hint.contains("was not matched")), "{:?}", p.unknown);
}

#[test]
fn a_valid_mark_is_priced_even_when_the_estimate_is_impossible() {
    let r = reply(&[item("wontons", 420.0, 5000.0, "estimate", Some("Northwind Pork Wontons"), Some(24.0))]);
    let p = pip_model::parse_saved("24 wontons", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].food.source, "label");
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn parse_without_foods_ignores_marks() {
    let p = pip_model::parse("24 wontons", &marked("wontons", "Northwind Pork Wontons", Some(24.0))).unwrap();
    assert_eq!(p.items.len(), 1);
    assert!(p.items[0].estimated);
    assert_ne!(p.items[0].food.source, "label");
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn the_prompt_lists_saved_foods_and_the_whole_message() {
    let photo = vec![PathBuf::from("/tmp/plate.jpg")];
    let p = pip_model::prompt_saved("24 wontons, rice 150g", &photo, &[w(None), b()]);
    assert!(p.contains("Saved foods"), "{p}");
    assert!(p.contains("Northwind Pork Wontons (140 g \u{b7} 8 pc); Northwind Rice Bowl (250 g)"), "{p}");
    assert!(p.contains("Message: \"24 wontons, rice 150g\""), "{p}");
    assert!(p.contains("\"saved\"") && p.contains("\"count\""), "{p}");
    assert!(!pip_model::prompt("", &photo).contains("Saved foods"));
    assert!(!pip_model::prompt_saved("", &photo, &[]).contains("Saved foods"));
    let _ = foods::serving_text(&w(None));
}

// ---- round 8: nothing the reader sends can beat his own text ----

const WON: &str = "Northwind Pork Wontons";

#[test]
fn one_saved_food_marked_twice_is_priced_once() {
    let r = reply(&[item("wontons", 420.0, 164.0, "estimate", Some(WON), Some(24.0)), item("plate wontons", 140.0, 164.0, "estimate", Some(WON), None)]);
    let p = pip_model::parse_saved("24 wontons", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1, "{}", p.items.len());
    assert_eq!(nutrition::item_line(&p.items[0]), "420 g Northwind Pork Wontons (24 pc, label) \u{b7} 690 kcal");
    assert_eq!(p.unknown.len(), 1, "{:?}", p.unknown);
    assert!(p.unknown[0].hint.contains("marked twice by the reader; counted once"), "{:?}", p.unknown);
}

#[test]
fn a_typed_name_sets_the_count_whatever_the_reader_sends() {
    let plain = |c: Option<f64>| marked("wontons", WON, c);
    let as_string = reply(&[item("wontons", 420.0, 164.0, "estimate", Some(WON), None).replace("\"saved\"", "\"count\":\"24\",\"saved\"")]);
    for r in [plain(Some(8.0)), plain(None), as_string] {
        let p = pip_model::parse_saved("24 wontons", &r, &[w(None)]).unwrap();
        assert_eq!(p.items.len(), 1);
        assert_eq!(nutrition::item_line(&p.items[0]), "420 g Northwind Pork Wontons (24 pc, label) \u{b7} 690 kcal", "{r}");
        assert!(!p.items[0].assumed, "{r}");
    }
}

#[test]
fn a_bare_typed_name_ignores_a_count_typed_for_another_food() {
    let r = marked("wontons", WON, Some(2.0));
    let p = pip_model::parse_saved("2 eggs, wontons", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    let i = &p.items[0];
    assert!(i.assumed, "{}", nutrition::item_line(i));
    assert_eq!(i.unit, Unit::Portion);
}

#[test]
fn a_typed_weight_survives_a_renamed_reader_item() {
    let r = marked("Pork dumplings", WON, Some(24.0));
    let p = pip_model::parse_saved("wontons 200g", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!((p.items[0].grams, p.items[0].unit), (200.0, Unit::G));
    assert!(nutrition::item_line(&p.items[0]).contains("label"), "{}", nutrition::item_line(&p.items[0]));
}

#[test]
fn typed_servings_of_a_saved_food_are_counted() {
    let r = marked("rice bowl", "Northwind Rice Bowl", None);
    let p = pip_model::parse_saved("2 servings northwind rice bowl", &r, &[b()]).unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!(nutrition::item_line(&p.items[0]), "500 g Northwind Rice Bowl (2 servings, label) \u{b7} 800 kcal");
}

#[test]
fn a_count_typed_for_a_table_food_is_not_copied_onto_a_saved_food() {
    let r = marked("dumplings", WON, Some(2.0));
    let p = pip_model::parse_saved("2 eggs, dumplings", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    assert!(p.items[0].assumed, "{}", nutrition::item_line(&p.items[0]));
    assert_ne!(p.items[0].unit, Unit::Piece);
}

#[test]
fn a_typed_name_twice_is_refused_and_keeps_the_estimate() {
    let r = marked("wontons", WON, Some(24.0));
    let p = pip_model::parse_saved("24 wontons, 8 wontons", &r, &[w(None)]).unwrap();
    assert!(p.unknown.iter().any(|u| u.hint.contains("named more than once in your message")), "{:?}", p.unknown);
    assert!(p.items.iter().all(|i| i.food.source != "label"));
}

#[test]
fn a_typed_saved_food_the_reader_omits_is_named_under_not_counted() {
    let r = reply(&[item("egg", 100.0, 143.0, "estimate", None, None)]);
    let p = pip_model::parse_saved("24 wontons, 2 eggs", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.unknown.len(), 1, "{:?}", p.unknown);
    assert!(p.unknown[0].hint.contains("not priced from your saved label for Northwind Pork Wontons"), "{:?}", p.unknown);
    assert!(p.unknown[0].hint.contains("if a line above is this food, that is the reader's estimate; if not, it was not counted"), "{:?}", p.unknown);
}

#[test]
fn a_saved_food_priced_from_a_mark_is_not_also_named_as_omitted() {
    let r = marked("wontons", WON, Some(24.0));
    let p = pip_model::parse_saved("24 wontons", &r, &[w(None)]).unwrap();
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn the_prompt_says_to_list_each_saved_food_once_marked() {
    let p = pip_model::prompt_saved("24 wontons", &[], &[w(None)]);
    assert!(p.contains("List each saved food once, marked: never add a second, unmarked item for it, even when the photo shows it or you would name it differently."), "{p}");
}

// ---- round 9: marks are keyed on the food they resolve to; a count belongs to the food it was typed for ----

fn chicken() -> SavedFood {
    SavedFood {
        name: "Northwind Chicken Dumplings".to_string(),
        names: vec!["Northwind Chicken Dumplings".to_string(), "Chicken Dumplings".to_string()],
        serving_g: Some(150.0),
        serving_pc: None,
        kcal: Some(300.0),
        protein: Some(0.0),
        carbs: Some(0.0),
        fat: Some(0.0),
        last: None,
        saved: "2026-10-29".to_string(),
    }
}

#[test]
fn a_refused_saved_food_marked_twice_is_one_estimate() {
    let r = reply(&[item("wontons", 420.0, 164.0, "estimate", Some(WON), Some(24.0)), item("plate wontons", 420.0, 164.0, "estimate", Some(WON), None)]);
    let p = pip_model::parse_saved("2-3 wontons", &r, &[w(None)]).unwrap();
    let lines: Vec<String> = p.items.iter().map(nutrition::item_line).collect();
    assert_eq!(lines.iter().filter(|l| l.contains("ontons")).count(), 1, "{lines:?}");
    assert!(lines[0].contains("420 g") && lines[0].contains("689 kcal"), "{lines:?}");
    assert_eq!(p.unknown.len(), 2, "{:?}", p.unknown);
    assert_eq!(p.unknown.iter().filter(|u| u.hint.contains("marked twice by the reader; counted once")).count(), 1, "{:?}", p.unknown);
}

#[test]
fn a_count_typed_for_one_saved_food_is_not_copied_onto_another() {
    let r = marked("dumplings", "Northwind Chicken Dumplings", Some(24.0));
    let p = pip_model::parse_saved("24 wontons", &r, &[w(None), chicken()]).unwrap();
    assert_eq!(p.items.len(), 1);
    let l = nutrition::item_line(&p.items[0]);
    assert!(!l.contains("24 pc"), "{l}");
    assert!(l.contains("default 1 serving") && l.contains("150 g") && l.contains("300 kcal"), "{l}");
    assert!(p.unknown.iter().any(|u| u.hint.contains("not priced from your saved label for Northwind Pork Wontons")), "{:?}", p.unknown);
}

#[test]
fn a_count_typed_under_an_unsaved_name_is_still_used() {
    let r = marked("wontons", WON, Some(24.0));
    let p = pip_model::parse_saved("24 potstickers", &r, &[w(None)]).unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!(nutrition::item_line(&p.items[0]), "420 g Northwind Pork Wontons (24 pc, label) \u{b7} 690 kcal");
}

#[test]
fn a_refused_typed_name_listed_as_estimate_gets_no_omitted_line() {
    let r = marked("wontons", WON, Some(24.0));
    let p = pip_model::parse_saved("24 wontons, 8 wontons", &r, &[w(None)]).unwrap();
    assert!(p.unknown.iter().all(|u| !u.hint.contains("not priced from your saved label") && !u.hint.contains("was not matched")), "{:?}", p.unknown);
}

#[test]
fn a_typed_name_listed_unmarked_is_not_called_unmatched() {
    let r = reply(&[item("pork wontons", 420.0, 164.0, "estimate", None, None)]);
    let p = pip_model::parse_saved("24 wontons", &r, &[w(None)]).unwrap();
    let lines: Vec<String> = p.items.iter().map(nutrition::item_line).collect();
    assert_eq!(lines.iter().filter(|l| l.to_lowercase().contains("wontons") && l.contains("(estimated)")).count(), 1, "{lines:?}");
    assert!(p.unknown.iter().any(|u| u.hint.contains("if a line above is this food, that is the reader's estimate")), "{:?}", p.unknown);
    assert!(p.unknown.iter().all(|u| !u.hint.contains("was not matched")), "{:?}", p.unknown);
}

#[test]
fn a_refused_typed_saved_food_the_reader_omits_is_named() {
    let r = reply(&[item("egg", 100.0, 143.0, "estimate", None, None)]);
    let p = pip_model::parse_saved("2-3 wontons, 2 eggs", &r, &[w(None)]).unwrap();
    assert!(p.unknown.iter().any(|u| u.text == "2-3 wontons"), "{:?}", p.unknown);
}

#[test]
fn a_multiplier_over_twelve_keeps_the_reader_estimate() {
    for text in ["wontons x24", "24x wontons", "wontons x 24"] {
        let p = pip_model::parse_saved(text, &marked("Wontons", "Wontons", Some(24.0)), &[w(None)]).unwrap();
        assert_eq!(p.items.len(), 1, "{text}: {:?}", p.unknown);
        assert_eq!(p.items[0].grams, 420.0, "{text}");
        assert!(p.items[0].estimated, "{text}");
        assert_ne!(p.items[0].food.source, "label", "{text}");
        assert_eq!(p.unknown.iter().filter(|u| u.hint.contains("only x1 to x12")).count(), 1, "{text}: {:?}", p.unknown);
    }
}

#[test]
fn an_ambiguous_alias_marked_twice_is_one_estimate() {
    let mut a = w(None);
    a.name = "Acme Dumplings".to_string();
    a.names = vec!["Acme Dumplings".to_string(), "Dumplings".to_string()];
    let mut c = w(None);
    c.name = "Northwind Dumplings".to_string();
    c.names = vec!["Northwind Dumplings".to_string(), "Dumplings".to_string()];
    let r = reply(&[
        item("Dumplings", 200.0, 200.0, "estimate", Some("Dumplings"), None),
        item("Dumplings", 200.0, 200.0, "estimate", Some("Dumplings"), None),
    ]);
    let p = pip_model::parse_saved("2 dumplings", &r, &[a, c]).unwrap();
    assert_eq!(p.items.len(), 1, "{:?}", p.unknown);
    assert_eq!(p.items[0].grams, 200.0);
    assert!(p.items[0].estimated);
    assert_eq!(p.unknown.iter().filter(|u| u.hint == "marked twice by the reader; counted once").count(), 1, "{:?}", p.unknown);
    assert_eq!(p.unknown.iter().filter(|u| u.hint.contains("more than one saved food")).count(), 1, "{:?}", p.unknown);
}
