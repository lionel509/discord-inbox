//! Breaker tests for PR #114 (Pip v2 summaries and charts). Scratch dirs live under the system temp dir.
use chrono::NaiveDate;
use discord_inbox::daylog;
use discord_inbox::summary;
use std::fs;
use std::path::{Path, PathBuf};

const DAY: &str = "2026-10-26";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("breaker114-sum-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Nutrition");
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn at(day: &str, hh: u32, mm: u32) -> chrono::NaiveDateTime {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap().and_hms_opt(hh, mm, 0).unwrap()
}

/// A meal region in the same shape `write_meal` produces: open marker, heading, items, macros, close marker.
fn meal_block(id: u64, open: bool, kcal: u32, items: usize, name_len: usize) -> String {
    let mut s = String::new();
    if open {
        s += &format!("<!-- pip:meal {id} -->\n");
    }
    s += &format!("### 12:3{} · msg {id}\n", id % 10);
    for i in 0..items {
        let name = format!("{i} {}", "x".repeat(name_len));
        s += &format!("- {name} · 30 g · 40 kcal\n");
    }
    s += &format!("- macros: {kcal} kcal P 30 C 40 F 20 veg yes\n");
    s += &format!("<!-- /pip:meal {id} -->\n");
    s
}

fn write_note(dir: &Path, body: &str) {
    fs::write(daylog::note_path(dir, DAY), format!("# {DAY}\n\n{body}")).unwrap();
}

/// Break 2: the plan says weekly_due needs "any of week_days has a meal". The code sends on an empty week.
#[test]
fn weekly_summary_is_not_sent_for_an_empty_week() {
    let dir = scratch("emptyweek");
    assert_eq!(summary::weekly_due(&dir, at("2026-10-25", 22, 0)), None);
}

/// Break 3: send_rich cuts the Meals value to 1024 chars silently, so later meals vanish with no "+N more".
/// The Rich itself is over the field limit.
#[test]
fn day_card_meals_field_fits_the_1024_value_limit() {
    let dir = scratch("meals1024");
    let mut body = String::new();
    for id in 1..=20u64 {
        body += &meal_block(id, true, 100, 4, 40);
    }
    write_note(&dir, &body);
    let card = summary::day_card(&dir, DAY, 1800, 2400);
    let meals = card.fields.iter().find(|f| f.0 == "Meals").expect("Meals field");
    let n = meals.1.chars().count();
    assert!(n <= 1024, "Meals value is {n} chars, over the 1024 field limit");
}

/// Break 4: the bin's dm_all stopped at the first failed DM, then the daily marker was released, so the next
/// 60 s tick sent the card to user A again. send_each claims a marker per recipient, so user A is sent once.
#[tokio::test]
async fn a_failing_second_recipient_does_not_resend_to_the_first() {
    let dir = scratch("dupe");
    let marker = summary::marker_path(&dir, DAY);
    let ids = [1u64, 2];
    let mut sent: Vec<u64> = Vec::new();
    let r1 = summary::send_each(&marker, &ids, |uid| {
        sent.push(uid);
        async move { if uid == 2 { Err("user B: create_dm failed".to_string()) } else { Ok(()) } }
    })
    .await;
    assert!(r1.is_err(), "user B failed, so the call must report the error");
    assert!(!marker.exists(), "the day marker must stay unclaimed while a recipient is outstanding");
    let r2 = summary::send_each(&marker, &ids, |uid| {
        sent.push(uid);
        async move { if uid == 2 { Err("user B: create_dm failed".to_string()) } else { Ok(()) } }
    })
    .await;
    assert!(r2.is_err());
    assert_eq!(sent.iter().filter(|&&u| u == 1).count(), 1, "user A was sent the same daily card twice: {sent:?}");
    let r3 = summary::send_each(&marker, &ids, |uid| {
        sent.push(uid);
        async { Ok(()) }
    })
    .await;
    assert!(r3.is_ok());
    assert_eq!(sent, vec![1, 2, 2, 2], "after the first call only user B is sent");
    assert!(marker.exists(), "all recipients sent, so the day marker must now exist");
}
