//! U6: the `today`/`week` cards, the streak, and the 22:00 daily and Sunday weekly summary rules. Scratch dirs are `…/Private/Nutrition`.
use chrono::NaiveDate;
use discord_inbox::daylog::{self, Meal};
use discord_inbox::nutrition::Macros;
use discord_inbox::summary::{self, Targets};
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-v2-summary-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn meal_on(dir: &Path, day: &str, id: u64, kcal: f64) {
    let m = Meal {
        msg_id: id,
        time: "12:31".into(),
        lines: vec!["- 100 g salmon · 208 kcal".into()],
        macros: Macros { kcal, protein: 20.0, carbs: 10.0, fat: 5.0 },
        ..Default::default()
    };
    daylog::write_meal(dir, day, &m, 1800).unwrap();
}

fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
}

#[test]
fn week_card_lists_each_day_and_marks_the_gap() {
    let dir = scratch("week");
    meal_on(&dir, "2026-10-20", 1, 1500.0);
    meal_on(&dir, "2026-10-22", 2, 2100.0);
    meal_on(&dir, "2026-10-26", 3, 1900.0);
    let card = summary::week_card(&dir, "2026-10-26", 1800, 2400);
    let days = card.fields.iter().find(|(n, _, _)| n == "Days").map(|(_, v, _)| v.clone()).unwrap_or_default();
    assert!(days.contains("1500"), "{days}");
    assert!(days.contains("2100"), "{days}");
    assert!(days.contains("1900"), "{days}");
    assert!(days.contains("not logged"), "{days}");
    assert_eq!(card.image.as_ref().map(|(n, _)| n.as_str()), Some("week.png"));
}

#[test]
fn streak_counts_back_from_today() {
    let dir = scratch("streak");
    meal_on(&dir, "2026-10-26", 4, 1900.0);
    assert_eq!(summary::streak(&dir, "2026-10-26"), 1);
}

#[test]
fn targets_come_from_the_kcal_target() {
    let t = Targets::from_kcal(2400);
    assert_eq!((t.protein, t.carbs, t.fat), (120.0, 300.0, 80.0));
}

#[test]
fn daily_summary_fires_at_ten_pm_once_a_meal_is_logged() {
    let dir = scratch("daily");
    assert!(summary::daily_due(&dir, at(2026, 10, 26, 22, 0)).is_none(), "no meal, no summary");
    meal_on(&dir, "2026-10-26", 5, 1900.0);
    assert!(summary::daily_due(&dir, at(2026, 10, 26, 21, 59)).is_none(), "before 22:00");
    assert!(summary::daily_due(&dir, at(2026, 10, 26, 22, 0)).is_some());
    fs::write(summary::marker_path(&dir, "2026-10-26"), "").unwrap();
    assert!(summary::daily_due(&dir, at(2026, 10, 26, 22, 5)).is_none(), "already sent");
}

#[test]
fn weekly_summary_is_sunday_at_ten_pm() {
    let dir = scratch("weekly");
    meal_on(&dir, "2026-10-25", 6, 1900.0);
    assert!(summary::weekly_due(&dir, at(2026, 10, 25, 22, 0)).is_some(), "2026-10-25 is a Sunday");
    assert!(summary::weekly_due(&dir, at(2026, 10, 26, 22, 0)).is_none(), "Monday");
}

#[test]
fn weekly_summary_is_sent_once_per_sunday() {
    let dir = scratch("weeklyonce");
    meal_on(&dir, "2026-10-25", 7, 1900.0);
    assert!(summary::weekly_due(&dir, at(2026, 10, 25, 22, 0)).is_some());
    fs::write(summary::weekly_marker(&dir, "2026-10-25"), "sent\n").unwrap();
    assert!(summary::weekly_due(&dir, at(2026, 10, 25, 22, 30)).is_none(), "already sent this Sunday");
}

#[test]
fn a_failed_send_removes_its_marker() {
    let dir = scratch("marker");
    fs::create_dir_all(&dir).unwrap();
    let marker = summary::marker_path(&dir, "2026-10-26");
    let res = summary::send_marked(&marker, || Err("discord is down".to_string()));
    assert!(res.is_err());
    assert!(!marker.exists(), "a failed send must not block the retry");
}
