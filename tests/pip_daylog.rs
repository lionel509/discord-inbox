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
    assert!(text.contains("kcal: 691"), "{text}");
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
    assert!(text.contains("flags: none"), "{text}");
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
