//! U3: the 21:00 check-in. At most one send per day, none on a day with a meal, suggestions free of banned foods.
use chrono::NaiveDate;
use discord_inbox::checkin::{self, BANNED, SUGGESTIONS};
use discord_inbox::daylog::{self, Meal};
use discord_inbox::pip::PRESENCE;
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-checkin-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn at(day: u32, h: u32, m: u32) -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap().and_hms_opt(h, m, 0).unwrap()
}

fn meal(id: u64) -> Meal {
    Meal { msg_id: id, time: "12:00".into(), note: Some("lunch".into()), ..Default::default() }
}

fn ok() -> impl std::future::Future<Output = Result<(), String>> {
    async { Ok::<(), String>(()) }
}

#[test]
fn check_in_hour_is_nine_pm() {
    assert_eq!(checkin::HOUR, 21);
}

#[test]
fn nothing_is_due_before_nine() {
    let dir = scratch("early");
    assert_eq!(checkin::due(&dir, at(25, 20, 59)), None);
}

#[test]
fn due_at_nine_with_no_meal() {
    let dir = scratch("due");
    assert_eq!(checkin::due(&dir, at(25, 21, 0)), Some("2026-10-25".to_string()));
}

#[tokio::test]
async fn a_sent_check_in_is_never_sent_again_that_day() {
    let dir = scratch("once");
    let day = checkin::due(&dir, at(25, 21, 0)).unwrap();
    checkin::send(&dir, &day, |_t| ok()).await.unwrap();
    assert_eq!(checkin::due(&dir, at(25, 21, 1)), None);
    assert_eq!(checkin::due(&dir, at(25, 23, 59)), None);
}

#[tokio::test]
async fn next_day_is_due_again() {
    let dir = scratch("next");
    let day = checkin::due(&dir, at(25, 21, 0)).unwrap();
    checkin::send(&dir, &day, |_t| ok()).await.unwrap();
    assert_eq!(checkin::due(&dir, at(26, 21, 0)), Some("2026-10-26".to_string()));
}

#[test]
fn a_day_with_a_meal_is_not_due() {
    let dir = scratch("meal");
    daylog::write_meal(&dir, "2026-10-25", &meal(1), 1800).unwrap();
    assert_eq!(checkin::due(&dir, at(25, 21, 0)), None);
}

#[tokio::test]
async fn a_failed_send_releases_the_day_for_a_retry() {
    let dir = scratch("fail");
    let day = checkin::due(&dir, at(25, 21, 0)).unwrap();
    let r = checkin::send(&dir, &day, |_t| async { Err::<(), String>("discord 500".into()) }).await;
    assert!(r.is_err());
    assert_eq!(checkin::due(&dir, at(25, 21, 5)), Some("2026-10-25".to_string()));
}

#[tokio::test]
async fn random_clock_draws_send_at_most_once_a_day_and_never_on_a_meal_day() {
    let dir = scratch("property");
    // Three days; days 0 and 2 get a meal logged, day 1 does not.
    for (d, day) in ["2026-10-25", "2026-10-26", "2026-10-27"].iter().enumerate() {
        if d != 1 {
            daylog::write_meal(&dir, day, &meal(100 + d as u64), 1800).unwrap();
        }
    }
    // Deterministic xorshift so a failure reproduces.
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |n: u64| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s % n
    };
    let mut sent = [0u32; 3];
    for _ in 0..500 {
        let d = next(3) as usize;
        let (h, m) = (next(24) as u32, next(60) as u32);
        let now = at(25 + d as u32, h, m);
        if let Some(day) = checkin::due(&dir, now) {
            if checkin::send(&dir, &day, |_t| ok()).await.is_ok() {
                sent[d] += 1;
            }
        }
    }
    assert!(sent.iter().all(|&n| n <= 1), "sends per day: {sent:?}");
    assert_eq!(sent[0], 0, "day with a meal got a check-in");
    assert_eq!(sent[2], 0, "day with a meal got a check-in");
    assert_eq!(sent[1], 1, "a day with no meal and 500 draws should get exactly one check-in: {sent:?}");
}

#[test]
fn suggestions_are_the_fixed_twelve() {
    assert_eq!(SUGGESTIONS.len(), 12);
    assert!(SUGGESTIONS.iter().all(|s| !s.trim().is_empty()));
}

#[test]
fn suggestions_contain_no_banned_food_as_a_whole_word() {
    assert!(!SUGGESTIONS.is_empty(), "suggestions must exist");
    for s in SUGGESTIONS {
        let words: Vec<String> = s.to_lowercase().split(|c: char| !c.is_alphanumeric()).map(String::from).collect();
        for b in BANNED {
            assert!(!words.iter().any(|w| w == b), "suggestion {s:?} contains banned {b:?}");
        }
    }
}

#[test]
fn presence_is_persona_only() {
    assert!(!PRESENCE.is_empty(), "presence must be set");
    let words: Vec<String> = PRESENCE.to_lowercase().split(|c: char| !c.is_alphanumeric()).map(String::from).collect();
    for bad in ["food", "meal", "kcal", "calorie", "eat", "weight", "diet", "health", "nutrition"] {
        assert!(!words.iter().any(|w| w == bad), "presence {PRESENCE:?} names {bad}");
    }
}

#[test]
fn just_woke_is_false_for_a_normal_tick_and_true_after_a_sleep() {
    let t0 = at(26, 21, 0);
    assert!(!checkin::just_woke(t0, t0 + chrono::Duration::seconds(60)), "60 s is a normal tick");
    assert!(!checkin::just_woke(t0, t0 + chrono::Duration::seconds(120)), "120 s is still a normal tick");
    assert!(checkin::just_woke(t0, t0 + chrono::Duration::seconds(121)), "121 s means the laptop slept");
    assert!(checkin::just_woke(t0, t0 + chrono::Duration::hours(3)), "3 h means the laptop slept");
}
