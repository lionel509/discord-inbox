//! Breaker round 1, unit A (src/pip_model.rs) against PR #133 head b1ed25e.
//! Only the tests the review (#133, fix list) must see green; the tolerance and cross-food cases were dropped.
#![allow(dead_code, unused_imports)]
use discord_inbox::pip_model::{self, Reading, Source};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const KCAL: f64 = 120.0;

fn item_json(name: &str, grams: &str, source: &str) -> String {
    format!(
        r#"{{"name":"{name}","grams":{grams},"source":"{source}","kcal_100":{KCAL},"protein_100":20,"carbs_100":0,"fat_100":3,"veg":false}}"#
    )
}

fn stub(tag: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("breaker-133-a-{}-{tag}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join("helper.sh");
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn first_item(r: Option<discord_inbox::nutrition::Parsed>) -> discord_inbox::nutrition::Item {
    r.expect("parse returned None").items.into_iter().next().expect("no item")
}

// 1. A card number the model puts in an item NAME is written to the day note unredacted.
#[test]
fn card_number_in_item_name_is_not_logged() {
    let r = pip_model::enforce(
        "100 g chicken",
        "VISA 4111 1111 1111 1111 chicken",
        100.0,
        Source::Text,
        KCAL,
        20.0,
        0.0,
        3.0,
        false,
    );
    let shown = match r {
        Ok(item) => item.food.name.to_string(),
        Err(u) => u.text,
    };
    assert!(!shown.contains("4111"), "card number reached the log: {shown:?}");
}

// 2. The impossible-number path puts the raw name (card number and all) into Unknown.text, which the day note prints.
#[test]
fn card_number_in_rejected_item_is_not_logged() {
    let r = pip_model::enforce(
        "receipt",
        "Card 4111 1111 1111 1111",
        0.0,
        Source::Estimate,
        KCAL,
        0.0,
        0.0,
        0.0,
        false,
    );
    let u = r.expect_err("grams 0 must be rejected");
    assert!(!u.text.contains("4111"), "card number in Unknown.text: {:?}", u.text);
}

// 3. "scale" is trusted with no photo at all: a reply with zero photos yields a measured (scale) line.
#[test]
fn scale_claim_without_any_photo_is_not_trusted() {
    let body = format!(
        "cat >/dev/null\nprintf '%s' '{{\"items\":[{}],\"unreadable\":[]}}'",
        item_json("chicken", "350", "scale")
    );
    let cmd = stub("nophoto", &body);
    let r = pip_model::read(&cmd, "100 g chicken", &[], Duration::from_secs(5));
    match r {
        Reading::Read(p) => {
            let it = p.items.first().expect("no item");
            assert!(!it.scale, "zero-photo reply was logged as a scale reading: {} g (scale)", it.grams);
        }
        Reading::Label(_) => panic!("a meal reply is not a label"),
        Reading::Down => panic!("helper reply was Down"),
    }
}

// 7. Prose after the JSON that contains a brace makes the whole reply Down.
#[test]
fn brace_in_trailing_prose_does_not_drop_the_reply() {
    let out = format!(
        "```json\n{{\"items\":[{}],\"unreadable\":[]}}\n```\nI weighed by eye {{roughly}}.",
        item_json("egg", "50", "estimate")
    );
    assert!(pip_model::parse("1 egg", &out).is_some(), "valid JSON followed by prose with braces was dropped");
}

// 8. Model echoes the example object from the prompt before its real answer.
#[test]
fn echoed_example_before_answer_still_parses() {
    let out = format!(
        "{{\"items\":[{}],\"unreadable\":[]}}\n{{\"items\":[{}],\"unreadable\":[]}}",
        item_json("cooked egg noodles", "100", "estimate"),
        item_json("egg", "50", "text")
    );
    assert!(pip_model::parse("100 grams of egg", &out).is_some(), "echoed example plus answer was dropped");
}

// 9. Silent drop: 25 valid items keep 20 and say nothing about the other 5.
#[test]
fn overflow_items_are_reported_not_silently_dropped() {
    let items: Vec<String> = (0..25).map(|i| item_json(&format!("food{i}"), "50", "estimate")).collect();
    let p = pip_model::parse("", &format!("{{\"items\":[{}]}}", items.join(","))).expect("parse");
    assert_eq!(p.items.len() + p.unknown.len(), 25, "5 of 25 items vanished with no note");
}

#[allow(dead_code)]
fn unused(_: &Path) {}

// 6. Source spoof across foods: the typed 100 belongs to chicken, but cabbage claims it and is logged as typed.
#[test]
fn text_weight_for_another_food_is_flagged() {
    let p = pip_model::parse(
        "100 grams of chicken",
        &format!("{{\"items\":[{}]}}", item_json("cabbage", "100", "text")),
    )
    .expect("parse");
    let it = &p.items[0];
    assert!(it.estimated, "cabbage 100 g logged as typed; the only 100 g in the text is for chicken");
}
