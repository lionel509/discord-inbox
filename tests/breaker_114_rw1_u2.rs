//! Round-1 rework (U2): stale owed lines after a replay, and the owed-day filter. Made-up fixtures only.
use discord_inbox::allowlist;
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
const T1925: &str = "2026-10-26T19:25:00-04:00";
const T1935: &str = "2026-10-26T19:35:00-04:00";
const T1940: &str = "2026-10-26T19:40:00-04:00";
const T1945: &str = "2026-10-26T19:45:00-04:00";

struct Msg {
    id: u64,
    author: u64,
    received: String,
    text: String,
    files: Vec<String>,
    replies: Mutex<Vec<String>>,
}

impl Msg {
    fn new(id: u64, received: &str, text: &str) -> Msg {
        Msg { id, author: MAIN, received: received.into(), text: text.into(), files: vec![], replies: Mutex::new(Vec::new()) }
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
    let root = std::env::temp_dir().join(format!("breaker114r1u2-{}-{name}", std::process::id()));
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

/// Break 1: a crash after the meal is written but before its owed line is cleared. The replay finds the meal
/// already logged and returns `Skipped` before `paid` runs, so the line stays. Every later undo that day is then
/// refused ("hasn't saved yet") for a meal that is saved.
#[tokio::test]
async fn replay_of_a_saved_meal_clears_its_owed_line() {
    let nut = scratch("b1-replay-saved");
    assert_eq!(run(&nut, &Msg::new(100, T19, "salmon 100g")).await, Outcome::Saved);
    // Crash window: the note has msg 100, the owed line was never cleared.
    fs::write(nut.join(".pip-owed"), "100 2026-10-26 19:30\n").unwrap();
    assert_eq!(run(&nut, &Msg::new(100, T19, "salmon 100g")).await, Outcome::Skipped);
    assert!(!nut.join(".pip-owed").exists(), "a replay that finds its meal already saved must clear the owed line");
    let undo = Msg::new(200, T1945, "undo");
    assert_eq!(run(&nut, &undo).await, Outcome::Saved);
    let replies = undo.replies.lock().unwrap().clone();
    assert!(replies.iter().any(|r| r.contains("Undid")), "undo was refused for a saved meal: {replies:?}");
    assert_eq!(live_meals_on(&nut, DAY), Vec::<u64>::new());
}

/// Break 2: a crash after `fix` claimed its id in the command ledger (and owed it) but before the replacement
/// meal was written. The replay returns `Skipped` at the top of `handle` (the ledger has the id), so the owed
/// line for the fix is never cleared and blocks undo and fix for the rest of the day.
#[tokio::test]
async fn replay_of_a_claimed_fix_clears_its_owed_line() {
    let nut = scratch("b2-replay-fix");
    assert_eq!(run(&nut, &Msg::new(100, T1925, "salmon 100g")).await, Outcome::Saved);
    assert_eq!(run(&nut, &Msg::new(110, T1935, "cooked rice 180g")).await, Outcome::Saved);
    // Crash window: ledger has 130 and the owed line for 130 was written before the claim.
    fs::write(nut.join(".pip-commands"), "130\n").unwrap();
    fs::write(nut.join(".pip-owed"), "130 2026-10-26 19:40\n").unwrap();
    assert_eq!(run(&nut, &Msg::new(130, T1940, "fix salmon 120g")).await, Outcome::Skipped);
    let undo = Msg::new(140, T1945, "undo");
    assert_eq!(run(&nut, &undo).await, Outcome::Saved);
    let replies = undo.replies.lock().unwrap().clone();
    assert!(replies.iter().any(|r| r.contains("Undid")), "undo was refused by a stale owed line: {replies:?}");
    assert_eq!(live_meals_on(&nut, DAY), vec![100]);
}

/// Finding 9: an owed line from an earlier day does not block today's undo, and the next `owe` today drops it.
#[tokio::test]
async fn owed_line_from_yesterday_neither_blocks_undo_nor_survives_owe() {
    let nut = scratch("f9-yesterday-owed");
    assert_eq!(run(&nut, &Msg::new(100, T19, "salmon 100g")).await, Outcome::Saved);
    fs::write(nut.join(".pip-owed"), "90 2025-10-25 21:00\n").unwrap();
    let undo = Msg::new(120, T1945, "undo");
    assert_eq!(run(&nut, &undo).await, Outcome::Saved);
    let replies = undo.replies.lock().unwrap().clone();
    assert!(replies.iter().any(|r| r.contains("Undid")), "yesterday's owed line blocked today's undo: {replies:?}");
    assert_eq!(live_meals_on(&nut, DAY), Vec::<u64>::new());
    daylog::owe(&nut, 130, DAY, "19:40").unwrap();
    let owed = fs::read_to_string(nut.join(".pip-owed")).unwrap();
    assert_eq!(owed, "130 2026-10-26 19:40\n", "owe must drop lines from earlier days");
}
