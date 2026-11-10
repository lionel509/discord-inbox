//! Breaker 114 (commands, allowlist, day-note writes, undo/fix). Scratch dirs live under the OS temp dir; no real notes are touched.
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

const MAIN: u64 = 100000000000000001;
const DAY: &str = "2026-10-26";
const T19: &str = "2026-10-26T19:30:00-04:00";

struct Msg {
    id: u64,
    author: u64,
    received: String,
    text: String,
    files: Vec<String>,
    fail_download: bool,
    yield_typing: bool,
    replies: Mutex<Vec<String>>,
}

impl Msg {
    fn new(id: u64, received: &str, text: &str) -> Msg {
        Msg {
            id,
            author: MAIN,
            received: received.into(),
            text: text.into(),
            files: Vec::new(),
            fail_download: false,
            yield_typing: false,
            replies: Mutex::new(Vec::new()),
        }
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
        if self.fail_download {
            Err("simulated download failure".into())
        } else {
            Ok(vec![0x89, b'P', b'N', b'G'])
        }
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
    async fn typing(&self) {
        if self.yield_typing {
            tokio::task::yield_now().await;
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("breaker114-{}-{name}", std::process::id()));
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
    daylog::meals(nut, day).iter().map(|m| m.msg_id).collect()
}

fn live_meals(nut: &Path) -> Vec<u64> {
    live_meals_on(nut, DAY)
}

fn note(nut: &Path) -> String {
    fs::read_to_string(daylog::note_path(nut, DAY)).unwrap_or_default()
}

fn checkin_due_at_2105(nut: &Path) -> bool {
    let at = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap().and_hms_opt(21, 5, 0).unwrap();
    checkin::due(nut, at).is_some()
}

// BREAK 1: the same undo message reaches pip::handle twice (live handler + catch-up) before either finishes.
// Both pass the command_seen check, then both undo. The ledger is only read, not checked-and-written under the lock.
#[tokio::test]
async fn same_undo_delivered_twice_undoes_once() {
    let nut = scratch("dup-undo");
    assert_eq!(run(&nut, &Msg::new(1, T19, "salmon 100g")).await, Outcome::Saved);
    assert_eq!(run(&nut, &Msg::new(2, T19, "cooked rice 180g")).await, Outcome::Saved);
    let mut u1 = Msg::new(3, T19, "undo");
    u1.yield_typing = true;
    let mut u2 = Msg::new(3, T19, "undo");
    u2.yield_typing = true;
    let c = cfg(&nut);
    let a = allow();
    let (_x, _y) = tokio::join!(pip::handle(&a, &c, &u1), pip::handle(&a, &c, &u2));
    assert_eq!(live_meals(&nut), vec![1], "one undo should leave meal 1; note:\n{}", note(&nut));
}

// BREAK 2: non-food messages that contain a digit (or a word that is not food) become meal blocks, which
// suppresses the 21:00 check-in.
#[tokio::test]
async fn non_food_messages_are_notes_and_keep_checkin_due() {
    let texts = ["test", "hi", "ok", "thanks", "slept 8 hours", "test 5", "room 101", "ok 2", "gym 45 min"];
    let mut bad = Vec::new();
    for (i, t) in texts.iter().enumerate() {
        let nut = scratch(&format!("nonfood-{i}"));
        run(&nut, &Msg::new(10 + i as u64, T19, t)).await;
        let meals = live_meals(&nut);
        let due = checkin_due_at_2105(&nut);
        if !meals.is_empty() || !due {
            bad.push(format!("{t:?}: meals={meals:?} checkin_due={due}"));
        }
    }
    assert!(bad.is_empty(), "non-food messages:\n{bad:#?}");
}

// BREAK 3: `fix` with a photo records the command, then the photo download fails. The ledger now says the
// message was seen, so the catch-up retry is Skipped and the fix is lost for good (the old meal stays).
#[tokio::test]
async fn fix_with_photo_survives_a_failed_download_retry() {
    let nut = scratch("fix-retry");
    assert_eq!(run(&nut, &Msg::new(1, T19, "salmon 100g")).await, Outcome::Saved);
    let mut first = Msg::new(2, T19, "fix cooked rice 180g");
    first.files = vec!["p.jpg".into()];
    first.fail_download = true;
    assert_eq!(run(&nut, &first).await, Outcome::Failed);
    let mut retry = Msg::new(2, T19, "fix cooked rice 180g");
    retry.files = vec!["p.jpg".into()];
    let out = run(&nut, &retry).await;
    assert_eq!(out, Outcome::Saved, "retry of a failed fix should apply it");
    assert_eq!(live_meals(&nut), vec![2], "fix should replace meal 1; note:\n{}", note(&nut));
}
