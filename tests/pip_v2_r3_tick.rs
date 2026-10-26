//! Round-3 rework (U0): check-in and card semantics through `checkin::tick`. Made-up fixtures only.
use chrono::NaiveDate;
use discord_inbox::checkin;
use discord_inbox::daylog::{self, Meal};
use discord_inbox::rich::Rich;
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-v2-r3-tick-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn at(day: u32, h: u32, m: u32) -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap().and_hms_opt(h, m, 0).unwrap()
}

fn meal(id: u64) -> Meal {
    Meal { msg_id: id, time: "12:00".into(), note: Some("lunch".into()), ..Default::default() }
}

#[tokio::test]
async fn tick_sends_each_id_its_checkin_once_and_retries_only_the_failed_one() {
    let dir = scratch("retry");
    let now = at(26, 21, 30);
    let ids = [11u64, 22];
    let mut got: Vec<(u64, String)> = Vec::new();
    for _ in 0..3 {
        checkin::tick(&dir, now, &ids, 1800, 2400, |uid, r: Rich| {
            got.push((uid, r.text.clone()));
            let ok = uid != 22;
            async move {
                if ok {
                    Ok(())
                } else {
                    Err("dm closed".to_string())
                }
            }
        })
        .await;
    }
    let to11: Vec<&(u64, String)> = got.iter().filter(|(u, _)| *u == 11).collect();
    assert_eq!(to11.len(), 1, "11 must get its check-in exactly once, got {got:?}");
    assert!(to11[0].1.contains("nothing logged"), "11's text: {}", to11[0].1);
    assert_eq!(got.iter().filter(|(u, _)| *u == 22).count(), 3, "22 must be retried on each failed tick");

    let mut second: Vec<(u64, String)> = Vec::new();
    checkin::tick(&dir, now, &ids, 1800, 2400, |uid, r: Rich| {
        second.push((uid, r.text.clone()));
        async { Ok::<(), String>(()) }
    })
    .await;
    assert_eq!(second.iter().map(|x| x.0).collect::<Vec<_>>(), vec![22], "only 22 is owed after an all-ok tick");

    let mut third: Vec<u64> = Vec::new();
    checkin::tick(&dir, now, &ids, 1800, 2400, |uid, _r: Rich| {
        third.push(uid);
        async { Ok::<(), String>(()) }
    })
    .await;
    assert!(third.is_empty(), "nothing is owed after everyone got it, sent {third:?}");
    assert_eq!(checkin::due(&dir, now), None, "the day is done once every id has its check-in");
}

#[tokio::test]
async fn tick_sends_the_day_card_once_per_id_after_ten() {
    let dir = scratch("card");
    daylog::write_meal(&dir, "2026-10-26", &meal(500), 1800).unwrap();
    let now = at(26, 22, 5);
    let ids = [11u64, 22];
    let mut all: Vec<(u64, Rich)> = Vec::new();
    for _ in 0..2 {
        checkin::tick(&dir, now, &ids, 1800, 2400, |uid, r: Rich| {
            all.push((uid, r));
            async { Ok::<(), String>(()) }
        })
        .await;
    }
    for id in ids {
        let mine: Vec<&(u64, Rich)> = all.iter().filter(|(u, _)| *u == id).collect();
        assert_eq!(mine.len(), 1, "{id} must get the day card exactly once, got {} sends", mine.len());
        assert!(!mine[0].1.title.is_empty(), "{id}'s card has no title");
        assert!(!mine[0].1.text.contains("nothing logged"), "{id} got a check-in on a day with a meal");
    }
}
