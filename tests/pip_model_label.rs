//! #146: the reader's label reply (`pip_model::parse_label`) and its prompt rule. No model is run; the reply is a string.
use discord_inbox::pip_model::{self, Label};

const LABEL: &str = r#"{"label":{"name":"Northwind Pork Wontons","serving_g":140,"serving_pc":8,"kcal":230,"protein":9,"carbs":28,"fat":null},"items":[],"unreadable":[]}"#;

#[test]
fn a_label_reply_gives_its_fields_and_a_null_stays_none() {
    let l: Label = pip_model::parse_label(LABEL).unwrap();
    assert_eq!(l.name, "Northwind Pork Wontons");
    assert_eq!(l.serving_g, Some(140.0));
    assert_eq!(l.serving_pc, Some(8.0));
    assert_eq!(l.kcal, Some(230.0));
    assert_eq!(l.protein, Some(9.0));
    assert_eq!(l.fat, None);
}

#[test]
fn a_label_with_items_is_still_a_label() {
    let out = r#"{"label":{"name":"Northwind Pork Wontons","serving_g":140,"serving_pc":8,"kcal":230,"protein":9,"carbs":28,"fat":9},"items":[{"name":"wontons","grams":100,"source":"estimate","kcal_100":200,"protein_100":5,"carbs_100":5,"fat_100":5,"veg":false}],"unreadable":[]}"#;
    assert!(pip_model::parse_label(out).is_some());
}

#[test]
fn a_meal_reply_is_not_a_label() {
    let out = r#"{"items":[{"name":"cooked rice","grams":180,"source":"estimate","kcal_100":130,"protein_100":3,"carbs_100":28,"fat_100":0.3,"veg":false}],"unreadable":[]}"#;
    assert!(pip_model::parse_label(out).is_none());
}

#[test]
fn a_label_with_no_name_is_not_a_label() {
    let out = r#"{"label":{"name":"","serving_g":140,"serving_pc":8,"kcal":230,"protein":9,"carbs":28,"fat":9},"items":[],"unreadable":[]}"#;
    assert!(pip_model::parse_label(out).is_none());
}

#[test]
fn the_prompt_names_the_label_rule_and_its_limits() {
    let p = pip_model::prompt("", &[]);
    assert!(p.contains("label"), "{p}");
    assert!(p.contains("never a label"), "{p}");
}
