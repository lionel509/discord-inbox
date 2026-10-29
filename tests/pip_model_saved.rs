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
    let p = pip_model::parse_saved("24 wontons", &r, &[w(Some((24.0, Unit::Piece)))]).unwrap();
    assert_eq!(p.items.len(), 1);
    let i = &p.items[0];
    assert!(i.assumed);
    assert_eq!((i.qty, i.unit), (24.0, Unit::Piece));
    assert!(p.unknown.is_empty());
    let p = pip_model::parse_saved("24 wontons", &r, &[w(None)]).unwrap();
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
    assert_eq!(p.unknown.len(), 1);
    assert!(p.unknown[0].hint.contains("more than one saved food"), "{}", p.unknown[0].hint);
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
