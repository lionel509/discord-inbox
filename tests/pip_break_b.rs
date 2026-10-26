//! Breaker B (daylog + checkin + wiring) against PR #108 @ cfadd1c. Scratch dirs under the OS temp dir, named `…/Private/Nutrition`.
use chrono::{NaiveDate, NaiveDateTime};
use discord_inbox::allowlist;
use discord_inbox::checkin;
use discord_inbox::daylog;
use discord_inbox::ingest::{catch_up_each, History, Inbound};
use discord_inbox::pip::{Config, PipHandler};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

const MAIN: u64 = 100000000000000001;
const MAIN_CH: u64 = 5001;
const DAY: &str = "2026-10-26";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-break-b-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn cfg(dir: &Path) -> Config {
    Config { nutrition_dir: dir.to_path_buf(), recipes_dir: None, low_kcal: 1800 }
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

// ---- fake Discord history (same shape as tests/pip_ingest.rs) ----

#[derive(Clone)]
struct Spec {
    id: u64,
    received: &'static str,
    text: &'static str,
}

struct FakeMsg {
    spec: Spec,
}

impl Inbound for FakeMsg {
    fn author_id(&self) -> u64 {
        MAIN
    }
    fn msg_id(&self) -> u64 {
        self.spec.id
    }
    fn channel_id(&self) -> u64 {
        MAIN_CH
    }
    fn received(&self) -> String {
        self.spec.received.to_string()
    }
    fn content(&self) -> String {
        self.spec.text.to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        Vec::new()
    }
    fn live(&self) -> bool {
        false
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Err("no attachments".into())
    }
    async fn react(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    async fn unreact(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        Vec::new()
    }
    async fn reply(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

struct FakeHistory {
    msgs: Mutex<Vec<Spec>>,
}

impl History for FakeHistory {
    type Msg = FakeMsg;
    async fn dm_channel(&self, _user_id: u64) -> Result<u64, String> {
        // A real HTTP call takes time; the bin's check-in task runs on another worker during it.
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(MAIN_CH)
    }
    async fn page(&self, channel: u64, after: Option<u64>) -> Result<Vec<FakeMsg>, String> {
        if channel != MAIN_CH {
            return Ok(vec![]);
        }
        Ok(self
            .msgs
            .lock()
            .unwrap()
            .iter()
            .filter(|s| after.is_none_or(|a| s.id > a))
            .map(|s| FakeMsg { spec: s.clone() })
            .collect())
    }
    async fn fetch(&self, _channel: u64, msg_id: u64) -> Result<FakeMsg, String> {
        self.msgs
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == msg_id)
            .map(|s| FakeMsg { spec: s.clone() })
            .ok_or_else(|| "gone".to_string())
    }
}

// ---- break 1: a note without the `## Meals` heading loses every earlier meal ----

#[test]
fn break1_missing_meals_heading_drops_logged_meals() {
    let dir = scratch("heading");
    fs::create_dir_all(&dir).unwrap();
    // A day note where the `## Meals` heading is gone (hand-edited, or an older layout).
    let existing = "---\ndate: 2026-10-26\nkcal: 208\nprotein_g: 20\ncarbs_g: 0\nfat_g: 10\nmeals: 1\nveg: no\nflags: no-veg, low-kcal\n---\n# 2026-10-26\n**Total:** 208 kcal\n\n### 12:31 · msg 111\n- 100 g salmon · 208 kcal\n- macros: 208 kcal · P 20 g · C 0 g · F 10 g · veg no\n\n";
    fs::write(daylog::note_path(&dir, DAY), existing).unwrap();

    let meal = daylog::Meal {
        msg_id: 222,
        time: "18:05".into(),
        lines: vec!["- 100 g tofu · 76 kcal".into()],
        macros: discord_inbox::nutrition::Macros { kcal: 76.0, protein: 8.0, carbs: 2.0, fat: 4.0 },
        veg: true,
        ..Default::default()
    };
    assert_eq!(daylog::write_meal(&dir, DAY, &meal, 1800).unwrap(), daylog::Written::Logged);

    let text = fs::read_to_string(daylog::note_path(&dir, DAY)).unwrap();
    assert!(text.contains("· msg 111"), "earlier meal 111 was dropped:\n{text}");
    assert!(text.contains("· msg 222"), "new meal missing:\n{text}");
    assert!(daylog::logged(&dir, DAY, 111), "meal 111 is no longer logged, so a replay would log it again");
}

// ---- break 2: text the user wrote above `## Meals` is deleted on the next meal ----

#[test]
fn break2_text_above_meals_heading_is_erased() {
    let dir = scratch("prose");
    fs::create_dir_all(&dir).unwrap();
    let existing = "---\ndate: 2026-10-26\nkcal: 208\nprotein_g: 20\ncarbs_g: 0\nfat_g: 10\nmeals: 1\nveg: no\nflags: no-veg, low-kcal\n---\n# 2026-10-26\n**Total:** 208 kcal\n**Flags:** ⚠️ no veg\n\nFelt bloated after lunch, skipped the cabbage.\n\n## Meals\n\n### 12:31 · msg 111\n- 100 g salmon · 208 kcal\n- macros: 208 kcal · P 20 g · C 0 g · F 10 g · veg no\n\n";
    fs::write(daylog::note_path(&dir, DAY), existing).unwrap();

    let meal = daylog::Meal {
        msg_id: 222,
        time: "18:05".into(),
        lines: vec!["- 100 g tofu · 76 kcal".into()],
        macros: discord_inbox::nutrition::Macros { kcal: 76.0, protein: 8.0, carbs: 2.0, fat: 4.0 },
        veg: true,
        ..Default::default()
    };
    daylog::write_meal(&dir, DAY, &meal, 1800).unwrap();

    let text = fs::read_to_string(daylog::note_path(&dir, DAY)).unwrap();
    assert!(text.contains("Felt bloated after lunch, skipped the cabbage."), "user's own note was deleted:\n{text}");
}

// ---- break 3: the 21:00 tick fires before catch-up has logged the DM sent while the laptop slept ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn break3_checkin_sent_before_catch_up_logs_the_meal() {
    let dir = scratch("asleep");
    let allow = allow();
    let cfg = cfg(&dir);
    // Sent at 21:30 while the laptop was asleep; caught up only when Pip starts at 23:50.
    let hist = FakeHistory {
        msgs: Mutex::new(vec![Spec { id: 1000, received: "2026-10-26T21:30:00-04:00", text: "100g salmon, 1 tbsp oil" }]),
    };
    let sent = Arc::new(Mutex::new(Vec::<String>::new()));
    let now: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap().and_hms_opt(23, 50, 0).unwrap();

    // Mirrors the fixed bin's `ready`: catch-up runs first, then the check-in loop starts with its immediate first tick.
    catch_up_each(&allow, &dir, &hist, &PipHandler { allow: &allow, cfg: &cfg }).await;
    let (d2, s2) = (dir.clone(), sent.clone());
    let tick = tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_secs(60));
        iv.tick().await;
        if let Some(day) = checkin::due(&d2, now) {
            let s3 = s2.clone();
            checkin::send(&d2, &day, move |t: String| {
                s3.lock().unwrap().push(t);
                async { Ok::<(), String>(()) }
            })
            .await
            .unwrap();
        }
    });
    tick.await.unwrap();

    assert!(daylog::logged(&dir, DAY, 1000), "catch-up did not log the 21:30 DM");
    assert_eq!(
        sent.lock().unwrap().len(),
        0,
        "check-in was sent at 23:50 although a meal from 21:30 that day is in the history"
    );
}

// ---- break 4: two concurrent saves of the same photo (live + catch-up) can fail ----

#[test]
fn break4_concurrent_save_of_one_photo_can_fail() {
    let root = scratch("race");
    fs::create_dir_all(&root).unwrap();
    let bytes = vec![0xABu8; 64 * 1024];
    let mut failures = Vec::new();
    for round in 0..300 {
        let path = root.join(format!("{DAY}_1231_{round}_0.jpg"));
        let bar = Barrier::new(2);
        let results: Vec<Result<(), String>> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..2)
                .map(|_| {
                    let (p, b, bar) = (path.clone(), bytes.clone(), &bar);
                    s.spawn(move || {
                        bar.wait();
                        daylog::save_photo(&p, &b).map_err(|e| e.to_string())
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for r in results {
            if let Err(e) = r {
                failures.push(format!("round {round}: {e}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} of 600 same-photo saves (2 callers x 300 rounds) failed (handler would react FAILED), e.g. {:?}", failures.len(), failures.first());
}
