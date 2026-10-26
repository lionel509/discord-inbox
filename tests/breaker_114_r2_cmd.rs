//! Breaker 114 round 2 (commands: late undo and late fix). Scratch dirs live under the OS temp dir; no real notes are touched.
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

struct Msg {
    id: u64,
    author: u64,
    received: String,
    text: String,
    replies: Mutex<Vec<String>>,
}

impl Msg {
    fn new(id: u64, received: &str, text: &str) -> Msg {
        Msg { id, author: MAIN, received: received.into(), text: text.into(), replies: Mutex::new(Vec::new()) }
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
        Vec::new()
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
    let root = std::env::temp_dir().join(format!("breaker114r2-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Nutrition")
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn cfg(nut: &Path) -> Config {
    Config { nutrition_dir: nut.to_path_buf(), recipes_dir: None, low_kcal: 1800, kcal_target: 2400 }
}

async fn run(nut: &Path, m: &Msg) -> Outcome {
    pip::handle(&allow(), &cfg(nut), m).await
}

fn live_meals_on(nut: &Path, day: &str) -> Vec<u64> {
    daylog::meals(nut, day).iter().map(|m| m.msg_id).collect()
}

fn note_on(nut: &Path, day: &str) -> String {
    fs::read_to_string(daylog::note_path(nut, day)).unwrap_or_default()
}

// BREAK 1 (round 2): an undo sent between two meals arrives late, after the second meal is already in the note.
// The undo must drop the meal sent before it (meal 1), not the newest meal in the file (meal 3).
#[tokio::test]
async fn undo_replayed_late_removes_the_meal_sent_after_it() {
    let nut = scratch("late-undo");
    assert_eq!(run(&nut, &Msg::new(1, T19, "salmon 100g")).await, Outcome::Saved);
    assert_eq!(run(&nut, &Msg::new(3, T19, "cooked rice 180g")).await, Outcome::Saved);
    // The undo (id 2) was sent between the two meals; it arrives late.
    let _ = run(&nut, &Msg::new(2, T19, "undo")).await;
    assert_eq!(
        live_meals_on(&nut, DAY),
        vec![3],
        "undo sent before meal 3 must drop meal 1, not meal 3. note:\n{}",
        note_on(&nut, DAY)
    );
}

// Same shape for `fix`: a fix sent between two meals and arriving last replaces the meal sent before it.
// Meal 3 must stay as it was.
#[tokio::test]
async fn fix_replayed_late_replaces_the_meal_before_it() {
    let nut = scratch("late-fix");
    assert_eq!(run(&nut, &Msg::new(1, T19, "salmon 100g")).await, Outcome::Saved);
    assert_eq!(run(&nut, &Msg::new(3, T19, "cooked rice 180g")).await, Outcome::Saved);
    let before = daylog::meals(&nut, DAY).into_iter().find(|m| m.msg_id == 3).expect("meal 3 logged");
    let _ = run(&nut, &Msg::new(2, T19, "fix salmon 120g")).await;
    assert_eq!(
        live_meals_on(&nut, DAY),
        vec![2, 3],
        "fix sent before meal 3 must replace meal 1, leaving 2 and 3. note:\n{}",
        note_on(&nut, DAY)
    );
    let after = daylog::meals(&nut, DAY).into_iter().find(|m| m.msg_id == 3).expect("meal 3 still live");
    assert_eq!(before.lines, after.lines, "meal 3 must be unchanged");
    assert!((before.kcal - after.kcal).abs() < 0.01, "meal 3 kcal must be unchanged");
}
