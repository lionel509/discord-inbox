//! U2: the gym block in the day note. A gym block is Pip-owned, dedupes by msg id, and is never a meal.
use discord_inbox::daylog::{self, Meal, Written};
use discord_inbox::nutrition::Macros;
use std::fs;
use std::path::PathBuf;

const DAY: &str = "2026-10-12";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-gym-daylog-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn lines() -> Vec<String> {
    vec!["Dumbbell row · Cable row · reps 15 15 12".to_string()]
}

#[test]
fn write_gym_is_not_a_meal() {
    let dir = scratch("notmeal");
    let w = daylog::write_gym(&dir, DAY, 501, "18:00", "gym Day 1 · session 501", &lines()).unwrap();
    assert!(matches!(w, Written::Logged));

    assert!(!daylog::has_meal(&dir, DAY), "a gym block is not a meal");
    assert_eq!(daylog::totals(&dir, DAY, 1800).map_or(0.0, |t| t.macros.kcal), 0.0, "gym adds no kcal");
    assert!(daylog::logged(&dir, DAY, 501), "the msg id is indexed so replays skip it");

    let again = daylog::write_gym(&dir, DAY, 501, "18:00", "gym Day 1 · session 501", &lines()).unwrap();
    assert!(matches!(again, Written::Already), "second write of the same msg id is a no-op");

    let blocks = daylog::gym_blocks(&dir, DAY);
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].0, 501);
    assert_eq!(blocks[0].2, lines());
}

#[test]
fn gym_line_newlines_are_collapsed() {
    let dir = scratch("oneline");
    let multi = vec!["gym\nsneaky line".to_string()];
    daylog::write_gym(&dir, DAY, 502, "18:01", "gym Day 1 · session 502", &multi).unwrap();
    let blocks = daylog::gym_blocks(&dir, DAY);
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].2.len(), 1, "one line stays one line: {:?}", blocks[0].2);
}

#[test]
fn meals_ignore_gym_blocks() {
    let dir = scratch("meals");
    let meal = Meal {
        msg_id: 601,
        time: "12:00".to_string(),
        lines: vec!["100g salmon".to_string()],
        macros: Macros { kcal: 200.0, ..Default::default() },
        veg: false,
        recipe: None,
        unknown: vec![],
        note: None,
        photos: vec![],
    };
    daylog::write_meal(&dir, DAY, &meal, 1800).unwrap();
    daylog::write_gym(&dir, DAY, 602, "18:00", "gym Day 1 · session 602", &lines()).unwrap();

    let rows = daylog::meals(&dir, DAY);
    assert_eq!(rows.len(), 1, "the gym block is not listed as a meal");
    assert_eq!(daylog::totals(&dir, DAY, 1800).map_or(0.0, |t| t.macros.kcal), 200.0);
}
