//! Breaker 114 round 3 (commands: undo/fix bounds; check-in per recipient; DST). Scratch dirs live under the OS temp dir.
use chrono::NaiveDate;
use discord_inbox::allowlist;
use discord_inbox::checkin;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use discord_inbox::rich::Rich;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const MAIN: u64 = 100000000000000001;
const STRANGER: u64 = 100000000000000999;
const DAY: &str = "2026-10-26";
const T19: &str = "2026-10-26T19:30:00-04:00";

struct Msg {
    id: u64,
    author: u64,
    received: String,
    text: String,
    files: Vec<String>,
    fail: bool,
    replies: Mutex<Vec<String>>,
}

impl Msg {
    fn new(id: u64, received: &str, text: &str) -> Msg {
        Msg { id, author: MAIN, received: received.into(), text: text.into(), files: vec![], fail: false, replies: Mutex::new(Vec::new()) }
    }
    fn by(id: u64, author: u64, text: &str) -> Msg {
        Msg { id, author, received: T19.into(), text: text.into(), files: vec![], fail: false, replies: Mutex::new(Vec::new()) }
    }
    fn with_photo(id: u64, received: &str, text: &str, fail: bool) -> Msg {
        Msg { id, author: MAIN, received: received.into(), text: text.into(), files: vec!["p.png".into()], fail, replies: Mutex::new(Vec::new()) }
    }
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        self.author
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        5001
    }
    fn received(&self) -> String {
        self.received.clone()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.files.clone()
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        if self.fail {
            return Err("cdn 500".into());
        }
        Ok(vec![0x89, b'P', b'N', b'G'])
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
    async fn reply(&self, text: &str) -> Result<(), String> {
        self.replies.lock().unwrap().push(text.to_string());
        Ok(())
    }
    async fn reply_rich(&self, r: &Rich) -> Result<(), String> {
        self.replies.lock().unwrap().push(r.text.clone());
        Ok(())
    }
    async fn typing(&self) {}
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("breaker114r3-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Nutrition")
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn cfg(nut: &Path) -> Config {
    Config { nutrition_dir: nut.to_path_buf(), recipes_dir: None, foods_dir: None, low_kcal: 1800, kcal_target: 2400, model: None, model_timeout: std::time::Duration::from_secs(90) }
}

async fn run(nut: &Path, m: &Msg) -> Outcome {
    pip::handle(&allow(), &cfg(nut), m).await
}

fn live_meals_on(nut: &Path, day: &str) -> Vec<u64> {
    let mut v: Vec<u64> = daylog::meals(nut, day).iter().map(|m| m.msg_id).collect();
    v.sort();
    v
}

fn note_on(nut: &Path, day: &str) -> String {
    fs::read_to_string(daylog::note_path(nut, day)).unwrap_or_default()
}

#[tokio::test]
async fn undo_sent_after_a_deferred_meal_refuses_and_keeps_the_earlier_meal() {
    let nut = scratch("deferred-undo");
    assert_eq!(run(&nut, &Msg::new(5, T19, "salmon 100g")).await, Outcome::Saved);
    assert_eq!(run(&nut, &Msg::with_photo(10, T19, "cooked rice 180g", true)).await, Outcome::Failed);
    let undo = Msg::new(20, T19, "undo");
    let _ = run(&nut, &undo).await;
    assert_eq!(live_meals_on(&nut, DAY), vec![5], "undo 20 undid meal 5 while meal 10 was owed. note:\n{}", note_on(&nut, DAY));
    let said = undo.replies.lock().unwrap().join("\n");
    assert!(said.contains("19:30") && said.contains("hasn't saved"), "reply must name the owed meal: {said}");
    assert_eq!(run(&nut, &Msg::with_photo(10, T19, "cooked rice 180g", false)).await, Outcome::Saved);
    assert_eq!(run(&nut, &Msg::new(20, T19, "undo")).await, Outcome::Skipped, "a refused undo is never applied later");
    assert_eq!(live_meals_on(&nut, DAY), vec![5, 10]);
}

#[tokio::test]
async fn fix_sent_after_a_deferred_meal_refuses_and_keeps_the_earlier_meal() {
    let nut = scratch("deferred-fix");
    assert_eq!(run(&nut, &Msg::new(5, T19, "salmon 100g")).await, Outcome::Saved);
    assert_eq!(run(&nut, &Msg::with_photo(10, T19, "cooked rice 90g", true)).await, Outcome::Failed);
    let fix = Msg::new(20, T19, "fix cooked rice 180g");
    let _ = run(&nut, &fix).await;
    assert_eq!(live_meals_on(&nut, DAY), vec![5], "fix 20 replaced meal 5 while meal 10 was owed. note:\n{}", note_on(&nut, DAY));
    assert!(fix.replies.lock().unwrap().join("\n").contains("hasn't saved"));
    assert_eq!(run(&nut, &Msg::with_photo(10, T19, "cooked rice 90g", false)).await, Outcome::Saved);
    assert_eq!(run(&nut, &Msg::new(20, T19, "fix cooked rice 180g")).await, Outcome::Skipped);
    assert_eq!(live_meals_on(&nut, DAY), vec![5, 10]);
}

// BREAK C (check-in): the bin claims each id's marker before its send. A process that dies while the send to id 22
// is in flight leaves `.checkin-DAY` (the day marker) and `.checkin-DAY.dm-22` (claimed, never sent) on disk.
// On restart the 60 s loop asks `checkin::due`, which says the day is done, so id 22 never gets the check-in.
#[tokio::test]
async fn a_restart_mid_fanout_still_owes_the_second_id_its_checkin() {
    let dir = scratch("crash-due");
    let ids = [11u64, 22];
    // Tick 1: 11 is sent, then the send to 22 hangs. The timeout drops the future, the way a killed process would.
    let r = tokio::time::timeout(
        Duration::from_millis(300),
        checkin::send_all(&dir, DAY, &ids, |uid, _text: String| async move {
            if uid == 22 {
                std::future::pending::<()>().await;
            }
            Ok::<(), String>(())
        }),
    )
    .await;
    assert!(r.is_err(), "the fan-out to 22 must still be in flight when the process dies");
    // Restart: the loop's due check at 21:30 must still owe the check-in to 22.
    let now = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap().and_hms_opt(21, 30, 0).unwrap();
    assert_eq!(
        checkin::due(&dir, now),
        Some(DAY.to_string()),
        "22 never got the check-in, but the loop treats the day as done"
    );
}

// BREAK C2 (check-in): the same crash, but the retry is called directly with a working sender. 22 must be sent,
// because it never got the check-in. Its per-recipient marker was claimed before the cut-off send, so it is skipped.
#[tokio::test]
async fn a_retry_after_a_cut_off_send_reaches_the_id_that_was_cut_off() {
    let dir = scratch("crash-retry");
    let ids = [11u64, 22];
    let _ = tokio::time::timeout(
        Duration::from_millis(300),
        checkin::send_all(&dir, DAY, &ids, |uid, _text: String| async move {
            if uid == 22 {
                std::future::pending::<()>().await;
            }
            Ok::<(), String>(())
        }),
    )
    .await;
    let mut sent: Vec<u64> = Vec::new();
    let r = checkin::send_all(&dir, DAY, &ids, |uid, _text: String| {
        sent.push(uid);
        async { Ok::<(), String>(()) }
    })
    .await;
    assert!(r.is_ok());
    assert!(sent.contains(&22), "the retry never sent the check-in to 22; sent to {sent:?}");
}

