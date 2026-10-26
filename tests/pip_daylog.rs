//! U3: one markdown note per day. Scratch dirs are `…/Private/Nutrition`; nothing touches a real vault.
use discord_inbox::daylog::{self, Meal, Written};
use discord_inbox::nutrition::Macros;
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-daylog-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

const DAY: &str = "2026-10-25";

fn meal(id: u64, kcal: f64, veg: bool) -> Meal {
    Meal {
        msg_id: id,
        time: "12:31".into(),
        lines: vec!["- 100 g salmon · 208 kcal".into()],
        macros: Macros { kcal, protein: 20.0, carbs: 10.0, fat: 5.0 },
        veg,
        ..Default::default()
    }
}

#[test]
fn same_message_twice_is_one_block() {
    let dir = scratch("twice");
    let m = meal(1234567890, 400.0, true);
    assert_eq!(daylog::write_meal(&dir, DAY, &m, 1800).unwrap(), Written::Logged);
    assert_eq!(daylog::write_meal(&dir, DAY, &m, 1800).unwrap(), Written::Already);
    let text = fs::read_to_string(daylog::note_path(&dir, DAY)).unwrap();
    assert_eq!(text.matches("### ").count(), 1, "{text}");
}

#[test]
fn header_totals_are_the_sum_of_the_meals() {
    let dir = scratch("sum");
    daylog::write_meal(&dir, DAY, &meal(1, 400.0, true), 1800).unwrap();
    daylog::write_meal(&dir, DAY, &meal(2, 291.0, true), 1800).unwrap();
    let t = daylog::totals(&dir, DAY, 1800).expect("note exists");
    assert_eq!(t.meals, 2);
    assert!((t.macros.kcal - 691.0).abs() < 1e-9, "{}", t.macros.kcal);
    let text = fs::read_to_string(daylog::note_path(&dir, DAY)).unwrap();
    assert!(text.contains("**Total:** 691 kcal"), "{text}");
}

#[test]
fn flags_name_no_veg_and_low_kcal() {
    let dir = scratch("flags");
    daylog::write_meal(&dir, DAY, &meal(1, 900.0, false), 500).unwrap();
    assert_eq!(daylog::totals(&dir, DAY, 500).unwrap().flags, vec!["no-veg"]);

    let dir = scratch("flags-low");
    daylog::write_meal(&dir, DAY, &meal(1, 300.0, true), 500).unwrap();
    assert_eq!(daylog::totals(&dir, DAY, 500).unwrap().flags, vec!["low-kcal"]);

    let dir = scratch("flags-none");
    daylog::write_meal(&dir, DAY, &meal(1, 900.0, true), 500).unwrap();
    let t = daylog::totals(&dir, DAY, 500).unwrap();
    assert!(t.flags.is_empty(), "{:?}", t.flags);
    let text = fs::read_to_string(daylog::note_path(&dir, DAY)).unwrap();
    assert!(text.contains("**Flags:** none"), "{text}");
}

#[test]
fn low_kcal_threshold_reads_the_env_with_a_1800_default() {
    std::env::remove_var("PIP_LOW_KCAL");
    assert_eq!(daylog::low_kcal_threshold(), 1800);
    std::env::set_var("PIP_LOW_KCAL", "500");
    assert_eq!(daylog::low_kcal_threshold(), 500);
    std::env::remove_var("PIP_LOW_KCAL");
}

#[test]
fn a_missing_day_is_simply_absent() {
    let dir = scratch("missing");
    assert!(daylog::totals(&dir, DAY, 1800).is_none());
    assert!(!daylog::has_meal(&dir, DAY));
    assert!(!daylog::note_path(&dir, DAY).exists(), "no file may be created by a read");
}

#[test]
fn day_key_is_the_local_date_of_the_message() {
    assert_eq!(daylog::day_key("2026-10-25T23:50:00-04:00"), "2026-10-25");
}

#[test]
fn logged_is_true_only_after_the_meal_is_written() {
    let dir = scratch("logged");
    assert!(!daylog::logged(&dir, DAY, 42));
    daylog::write_meal(&dir, DAY, &meal(42, 100.0, true), 1800).unwrap();
    assert!(daylog::logged(&dir, DAY, 42));
    assert!(!daylog::logged(&dir, DAY, 43));
}

#[test]
fn photo_is_saved_beside_the_note_and_linked_from_it() {
    let dir = scratch("photo");
    let path = daylog::photo_path(&dir, DAY, "1231", 1234567890, 0, "IMG_0042.JPG");
    assert_eq!(path.extension().unwrap(), "jpg", "extension is lower-cased");
    assert_eq!(path.parent().unwrap(), dir.as_path(), "saved alongside the note");
    daylog::save_photo(&path, b"abc").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"abc");
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let mut m = meal(1234567890, 208.0, true);
    m.photos = vec![name.clone()];
    daylog::write_meal(&dir, DAY, &m, 1800).unwrap();
    let text = fs::read_to_string(daylog::note_path(&dir, DAY)).unwrap();
    assert!(text.contains(&format!("photo: ![[{name}]]")), "{text}");
}

#[test]
fn a_note_only_meal_counts_as_logged_with_zero_kcal() {
    let dir = scratch("note");
    let m = Meal { msg_id: 7, time: "19:05".into(), note: Some("ate out".into()), ..Default::default() };
    daylog::write_meal(&dir, DAY, &m, 1800).unwrap();
    assert!(daylog::has_meal(&dir, DAY));
    let t = daylog::totals(&dir, DAY, 1800).unwrap();
    assert_eq!(t.meals, 1);
    assert_eq!(t.macros.kcal, 0.0);
}

const NOTE_DAY: &str = "2026-10-26";

fn raw_note(dir: &std::path::Path, day: &str, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(daylog::note_path(dir, day), text).unwrap();
}

fn read_day(dir: &std::path::Path, day: &str) -> String {
    fs::read_to_string(daylog::note_path(dir, day)).unwrap()
}

#[test]
fn user_front_matter_stays_in_one_block() {
    let dir = scratch("user-fm");
    raw_note(
        &dir,
        NOTE_DAY,
        "---\ndate: 2026-10-26\ntags: [food]\naliases: [lunch-log]\n---\n# 2026-10-26\n**Total:** 200 kcal\n**Flags:** none\n\n## Meals\n\n### 12:00 · msg 1\n- rice\n- macros: 200 kcal · P 4 g · C 44 g · F 1 g · veg yes\n\n",
    );
    daylog::write_meal(&dir, NOTE_DAY, &meal(2, 150.0, true), 1800).unwrap();
    daylog::write_meal(&dir, NOTE_DAY, &meal(3, 120.0, true), 1800).unwrap();
    let out = read_day(&dir, NOTE_DAY);
    let lines: Vec<&str> = out.lines().collect();
    let fences: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| **l == "---").map(|(i, _)| i).collect();
    let tags: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| **l == "tags: [food]").map(|(i, _)| i).collect();
    assert_eq!(fences.len(), 2, "{out}");
    assert_eq!(tags.len(), 1, "{out}");
    assert!(fences[0] < tags[0] && tags[0] < fences[1], "{out}");
}

#[test]
fn user_title_is_kept() {
    let dir = scratch("user-title");
    raw_note(&dir, NOTE_DAY, "# Rough day\n\nslept badly\n");
    daylog::write_meal(&dir, NOTE_DAY, &meal(1, 300.0, true), 1800).unwrap();
    let out = read_day(&dir, NOTE_DAY);
    assert_eq!(out.matches("# Rough day").count(), 1, "{out}");
    assert_eq!(out.matches("slept badly").count(), 1, "{out}");
    // Pip adds a title only to a note it creates.
    assert_eq!(out.lines().filter(|l| *l == "# 2026-10-26").count(), 0, "{out}");
}

#[test]
fn leading_rule_block_survives_two_writes() {
    let dir = scratch("leading-rule");
    raw_note(&dir, NOTE_DAY, "---\nFelt bloated after lunch, skipped the cabbage.\n---\n\n");
    daylog::write_meal(&dir, NOTE_DAY, &meal(1, 300.0, true), 1800).unwrap();
    daylog::write_meal(&dir, NOTE_DAY, &meal(2, 200.0, true), 1800).unwrap();
    let out = read_day(&dir, NOTE_DAY);
    assert_eq!(out.matches("Felt bloated after lunch, skipped the cabbage.").count(), 1, "{out}");
}

#[test]
fn typed_heading_is_not_a_meal() {
    let dir = scratch("typed-heading");
    raw_note(&dir, NOTE_DAY, "### how I feel\nrough day\n");
    assert!(!daylog::has_meal(&dir, NOTE_DAY));
    daylog::write_meal(&dir, NOTE_DAY, &meal(1, 300.0, true), 1800).unwrap();
    assert!(daylog::has_meal(&dir, NOTE_DAY));
}

#[test]
fn pip_note_is_byte_exact() {
    let dir = scratch("golden");
    let a = Meal {
        msg_id: 1,
        time: "12:31".into(),
        lines: vec!["200 g salmon".into()],
        macros: Macros { kcal: 400.0, protein: 40.0, carbs: 0.0, fat: 20.0 },
        veg: true,
        ..Default::default()
    };
    let b = Meal {
        msg_id: 2,
        time: "19:05".into(),
        lines: vec!["1 cup cabbage".into()],
        macros: Macros { kcal: 291.0, protein: 10.0, carbs: 30.0, fat: 5.0 },
        veg: false,
        ..Default::default()
    };
    daylog::write_meal(&dir, NOTE_DAY, &a, 1800).unwrap();
    daylog::write_meal(&dir, NOTE_DAY, &b, 1800).unwrap();
    let expected = r#"# 2026-10-26

<!-- pip:totals -->
**Total:** 691 kcal · P 50 g · C 30 g · F 25 g · 2 meals · veg yes
**Flags:** ⚠️ low kcal (691 < 1800)
<!-- /pip:totals -->

## Meals

<!-- pip:meal 1 -->
### 12:31 · msg 1
- 200 g salmon
- macros: 400 kcal · P 40 g · C 0 g · F 20 g · veg yes
<!-- /pip:meal 1 -->

<!-- pip:meal 2 -->
### 19:05 · msg 2
- 1 cup cabbage
- macros: 291 kcal · P 10 g · C 30 g · F 5 g · veg no
<!-- /pip:meal 2 -->
"#;
    assert_eq!(read_day(&dir, NOTE_DAY), expected);
}
