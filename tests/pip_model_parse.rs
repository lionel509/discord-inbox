//! #130 unit A: the model's JSON becomes items only through `enforce`; typed weights are checked against his text.
use discord_inbox::pip_model::{self, Reading};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const IMPOSSIBLE: &str = "the reader gave an impossible number";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pip-model-parse-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A `/bin/sh` helper with the given body, executable.
fn helper(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("helper.sh");
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn item_json(name: &str, grams: f64, typed: bool, kcal: f64) -> String {
    format!(
        r#"{{"name":"{name}","grams":{grams},"typed":{typed},"kcal_100":{kcal},"protein_100":4.5,"carbs_100":25,"fat_100":2.1,"veg":false}}"#
    )
}

fn reply(items: &[String], unreadable: &[&str]) -> String {
    let u: Vec<String> = unreadable.iter().map(|s| format!("\"{s}\"")).collect();
    format!(r#"{{"items":[{}],"unreadable":[{}]}}"#, items.join(","), u.join(","))
}

#[test]
fn typed_weight_is_authoritative() {
    let text = "100 grams of cooked egg noodles 100 grams of chicken breast";
    let out = reply(
        &[item_json("cooked egg noodles", 100.0, true, 138.0), item_json("chicken breast", 120.0, true, 120.0)],
        &[],
    );
    let p = pip_model::parse(text, &out).expect("object with items");
    assert_eq!(p.items.len(), 2, "{:?}", p.unknown);
    let noodle = p.items.iter().find(|i| i.food.name.contains("noodle")).expect("noodles logged");
    let chicken = p.items.iter().find(|i| i.food.name.contains("chicken")).expect("chicken logged");
    assert!(!noodle.estimated && (noodle.grams - 100.0).abs() < 1e-9, "noodles: his typed 100 g, got {}", noodle.grams);
    assert!(!chicken.estimated && (chicken.grams - 100.0).abs() < 1e-9, "chicken: his typed 100 g, got {}", chicken.grams);
    assert!(p.items.iter().all(|i| (i.grams - 120.0).abs() > 1e-9), "the reader's 120 g is never logged for a typed food");
}

#[test]
fn no_typed_weight_is_estimated() {
    let out = reply(&[item_json("chicken breast", 100.0, false, 120.0)], &[]);
    let p = pip_model::parse("chicken breast", &out).unwrap();
    assert_eq!(p.items.len(), 1);
    assert!(p.items[0].estimated);
}

#[test]
fn known_food_uses_table_numbers() {
    let out = reply(&[item_json("egg", 50.0, false, 999.0)], &[]);
    let p = pip_model::parse("egg", &out).unwrap();
    assert_eq!(p.items[0].food.name, "egg");
    assert!((p.items[0].food.kcal - 143.0).abs() < 1e-9, "table kcal wins over the model's 999");
}

#[test]
fn impossible_numbers_are_not_counted() {
    let out = reply(
        &[
            item_json("butter", 10.0, false, 1500.0),
            item_json("salmon", 0.0, false, 208.0),
            item_json("cooked rice", 9000.0, false, 130.0),
            r#"{"name":"tofu","grams":100,"typed":false,"kcal_100":76,"protein_100":-1,"carbs_100":2,"fat_100":4,"veg":false}"#.to_string(),
        ],
        &[],
    );
    let p = pip_model::parse("butter salmon rice tofu", &out).unwrap();
    assert!(p.items.is_empty(), "{:?}", p.items.iter().map(|i| i.food.name).collect::<Vec<_>>());
    assert_eq!(p.unknown.len(), 4, "{:?}", p.unknown);
    assert!(p.unknown.iter().all(|u| u.hint == IMPOSSIBLE), "{:?}", p.unknown);
}

#[test]
fn prose_and_fences_around_json_parse() {
    let body = reply(&[item_json("salmon", 100.0, true, 208.0)], &[]);
    let fenced = format!("Here you go:\n```json\n{body}\n```\nHope that helps.");
    let p = pip_model::parse("salmon 100g", &fenced).expect("JSON inside prose and a fence");
    assert_eq!(p.items.len(), 1);
    assert!(pip_model::parse("salmon 100g", "Sorry, I cannot read that.").is_none());
    assert!(pip_model::parse("salmon 100g", r#"{"foo":1}"#).is_none(), "no items array");
}

#[test]
fn unreadable_is_redacted_and_capped() {
    let card = reply(&[], &["VISA 4111111111111111 noodles"]);
    let p = pip_model::parse("noodles", &card).unwrap();
    assert_eq!(p.unknown.len(), 1);
    assert!(!p.unknown[0].text.contains("4111"), "{:?}", p.unknown);

    let long = "x".repeat(200);
    let p = pip_model::parse("noodles", &reply(&[], &[long.as_str()])).unwrap();
    assert!(p.unknown[0].text.chars().count() <= 80, "{}", p.unknown[0].text.len());

    // 15 unreadable strings: at most 10 named, and one count line covers the rest (named + N == 15).
    let many: Vec<String> = (0..15).map(|i| format!("thing {i}")).collect();
    let many: Vec<&str> = many.iter().map(|s| s.as_str()).collect();
    let p = pip_model::parse("noodles", &reply(&[], &many)).unwrap();
    let named: Vec<&str> = p.unknown.iter().filter(|u| !u.text.starts_with("and ")).map(|u| u.text.as_str()).collect();
    assert!(named.len() <= 10, "at most 10 named: {:?}", p.unknown);
    let counted: usize = p
        .unknown
        .iter()
        .find(|u| u.text.starts_with("and ") && u.text.ends_with(" more unreadable"))
        .and_then(|u| u.text.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .expect("a count line for the rest");
    assert_eq!(named.len() + counted, 15, "every string is named or counted: {:?}", p.unknown);
    assert!(p.unknown.iter().all(|u| !u.text.is_empty()));

    let items: Vec<String> = (0..30).map(|_| item_json("salmon", 100.0, false, 208.0)).collect();
    let p = pip_model::parse("salmon", &reply(&items, &[])).unwrap();
    assert_eq!(p.items.len(), 20);
}

#[test]
fn read_passes_photos_as_args_and_prompt_on_stdin() {
    let dir = scratch("args");
    let args = dir.join("args.txt");
    let stdin = dir.join("stdin.txt");
    let body = format!(
        "printf '%s\\n' \"$@\" > '{}'\ncat > '{}'\necho '{}'",
        args.display(),
        stdin.display(),
        reply(&[], &[])
    );
    let cmd = helper(&dir, &body);
    let photos = vec![dir.join("a.jpg"), dir.join("b.jpg")];
    match pip_model::read(&cmd, "salmon 100g", &photos, Duration::from_secs(5)) {
        Reading::Read(_) => {}
        Reading::Down => panic!("helper printed JSON; it should be read"),
    }
    let got_args = fs::read_to_string(&args).unwrap();
    let got: Vec<&str> = got_args.lines().collect();
    assert_eq!(got, photos.iter().map(|p| p.to_str().unwrap()).collect::<Vec<_>>());
    let prompt = fs::read_to_string(&stdin).unwrap();
    assert!(prompt.contains("Photos: 2 attached"), "{prompt}");
    assert!(prompt.contains("salmon 100g"), "{prompt}");
    for p in &photos {
        assert!(prompt.contains(p.to_str().unwrap()), "{prompt}");
    }
}

#[test]
fn helper_failure_and_timeout_are_down() {
    let dir = scratch("down");
    let failing = helper(&dir, "cat >/dev/null\nexit 1");
    assert!(matches!(pip_model::read(&failing, "salmon", &[], Duration::from_secs(5)), Reading::Down));

    let slow = helper(&dir, "cat >/dev/null\nsleep 3\necho '{\"items\":[],\"unreadable\":[]}'");
    assert!(matches!(pip_model::read(&slow, "salmon", &[], Duration::from_secs(1)), Reading::Down));

    let junk = helper(&dir, "cat >/dev/null\necho 'not json'");
    assert!(matches!(pip_model::read(&junk, "salmon", &[], Duration::from_secs(5)), Reading::Down));
}
