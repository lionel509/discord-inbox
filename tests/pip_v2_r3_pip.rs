//! Round-3 rework (U0): owed messages and the undo/fix refusal, through `pip::handle`. Made-up fixtures only.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use discord_inbox::rich::Rich;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAIN: u64 = 100000000000000001;
const STRANGER: u64 = 100000000000000999;
const DAY: &str = "2026-10-26";
const T19: &str = "2026-10-26T19:30:00-04:00";
const SALMON: &str = "# Gochujang Honey Garlic Salmon\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n";

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
    Config { nutrition_dir: nut.to_path_buf(), recipes_dir: None, low_kcal: 1800, kcal_target: 2400, model: None, model_timeout: std::time::Duration::from_secs(90) }
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
async fn stranger_meal_writes_no_owed_line() {
    let nut = scratch("stranger-owed");
    assert_eq!(run(&nut, &Msg::by(30, STRANGER, "salmon 100g")).await, Outcome::Ignored);
    assert!(!nut.join(".pip-owed").exists(), "a stranger's message must not owe a meal");
}

#[tokio::test]
async fn a_saved_meal_leaves_nothing_owed() {
    let nut = scratch("saved-owed");
    assert_eq!(run(&nut, &Msg::new(30, T19, "salmon 100g")).await, Outcome::Saved);
    assert!(!nut.join(".pip-owed").exists(), "a saved meal must not leave a line in .pip-owed");
}

/// Deterministic xorshift (as in tests/pip_checkin.rs) so a failure reproduces.
fn rng(seed: u64) -> impl FnMut(u64) -> u64 {
    let mut s = seed;
    move |n: u64| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s % n
    }
}

/// I2: 60 generated sequences of meals, failed meals, and undos. An undo never removes a meal other than the one
/// the model says it should, and an undo sent while a meal is owed is refused and removes nothing.
#[tokio::test]
async fn generated_undo_sequences_never_undo_the_wrong_meal() {
    let mut next = rng(0x9E37_79B9_7F4A_7C15);
    let mut failures = Vec::new();
    for case in 0..60u64 {
        let nut = scratch(&format!("gen-{case}"));
        let mut live: Vec<u64> = Vec::new();
        let mut owed = false;
        for step in 0..6u64 {
            let id = 1000 + 10 * step;
            match next(4) {
                0 | 1 => {
                    let out = run(&nut, &Msg::new(id, T19, "salmon 100g")).await;
                    if out != Outcome::Saved {
                        failures.push(format!("case {case} step {step}: meal gave {out:?}"));
                    }
                    live.push(id);
                }
                2 => {
                    let out = run(&nut, &Msg::with_photo(id, T19, "cooked rice 100g", true)).await;
                    if out != Outcome::Failed {
                        failures.push(format!("case {case} step {step}: failed meal gave {out:?}"));
                    }
                    owed = true;
                }
                _ => {
                    let undo = Msg::new(id, T19, "undo");
                    let _ = run(&nut, &undo).await;
                    let said = undo.replies.lock().unwrap().join("\n");
                    if owed {
                        if !said.contains("hasn't saved") {
                            failures.push(format!("case {case} step {step}: undo while owed was not refused: {said:?}"));
                        }
                    } else {
                        live.pop();
                    }
                }
            }
            let got = live_meals_on(&nut, DAY);
            if got != live {
                failures.push(format!("case {case} step {step}: live {got:?}, model {live:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} steps went wrong:\n{}", failures.len(), failures.join("\n"));
}

/// A recipe line the typed text could not use is shown in the note, not swallowed. Needs U1 and U2 merged.
#[tokio::test]
async fn recipe_refusal_hint_reaches_the_note() {
    let nut = scratch("refusal-note");
    let recipes = nut.parent().unwrap().join("Private").join("Recipes");
    fs::create_dir_all(&recipes).unwrap();
    fs::write(recipes.join("Gochujang Honey Garlic Salmon.md"), SALMON).unwrap();
    let cfg = Config { nutrition_dir: nut.clone(), recipes_dir: Some(recipes), low_kcal: 1800, kcal_target: 2400, model: None, model_timeout: std::time::Duration::from_secs(90) };
    let m = Msg::new(40, T19, "gochujang honey garlic salmon 150");
    assert_eq!(pip::handle(&allow(), &cfg, &m).await, Outcome::Saved);
    let note = note_on(&nut, DAY);
    assert!(note.contains("add g"), "the refusal hint must reach the note:\n{note}");
    assert!(!note.contains("two foods"), "the old wording must be gone:\n{note}");
}
