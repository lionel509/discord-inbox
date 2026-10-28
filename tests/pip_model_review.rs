//! #133 review fixes: the meal reader through `pip::handle`, with a `/bin/sh` helper standing in for the model (`PIP_MODEL`).
//! Scratch dirs are `…/Private/Nutrition` under the temp dir, never a real vault.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use discord_inbox::pip_model;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAIN: u64 = 100000000000000001;
const MAIN_CH: u64 = 5001;

struct Msg {
    id: u64,
    received: String,
    text: String,
    files: Vec<String>,
    live: bool,
    typing_calls: Arc<AtomicUsize>,
    replies: Arc<Mutex<Vec<String>>>,
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        MAIN
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        MAIN_CH
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
    fn live(&self) -> bool {
        self.live
    }
    async fn typing(&self) {
        self.typing_calls.fetch_add(1, Ordering::SeqCst);
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(b"jpeg bytes".to_vec())
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
    async fn reply_rich(&self, r: &discord_inbox::rich::Rich) -> Result<(), String> {
        self.replies.lock().unwrap().push(format!("{:?}", r));
        Ok(())
    }
}

/// A fresh scratch nutrition dir, and the root above it (the helper, its reply and its call log live there).
fn world(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pip-model-review-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Nutrition");
    fs::create_dir_all(&dir).unwrap();
    (dir, root)
}

/// A `/bin/sh` helper that drains the prompt and prints `reply`.
fn stub(root: &Path, reply: &str) -> PathBuf {
    let reply_file = root.join("model-out");
    fs::write(&reply_file, reply).unwrap();
    let p = root.join("helper.sh");
    fs::write(&p, format!("#!/bin/sh\ncat >/dev/null\ncat '{}'\n", reply_file.display())).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn cfg(dir: &Path, model: Option<PathBuf>) -> Config {
    Config {
        nutrition_dir: dir.to_path_buf(),
        recipes_dir: None,
        low_kcal: 1800,
        kcal_target: 2400,
        model,
        model_timeout: Duration::from_secs(5),
    }
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn msg(id: u64, text: &str, live: bool) -> (Msg, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
    let typing_calls = Arc::new(AtomicUsize::new(0));
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg {
        id,
        received: "2026-10-27T19:30:00-04:00".to_string(),
        text: text.to_string(),
        files: Vec::new(),
        live,
        typing_calls: typing_calls.clone(),
        replies: replies.clone(),
    };
    (m, typing_calls, replies)
}

async fn run(c: &Config, m: &Msg) -> Outcome {
    pip::handle(&allow(), c, m).await
}

fn note(dir: &Path) -> String {
    fs::read_to_string(daylog::note_path(dir, "2026-10-27")).unwrap_or_default()
}

fn item_json(name: &str, grams: f64, source: &str, kcal: f64) -> String {
    format!(
        r#"{{"name":"{name}","grams":{grams},"source":"{source}","kcal_100":{kcal},"protein_100":4,"carbs_100":25,"fat_100":2,"veg":false}}"#
    )
}

fn reply_json(items: &str) -> String {
    format!("{{\"items\":[{items}],\"unreadable\":[]}}")
}

// Finding 3: a model food with no calories must not log as 0 kcal.
#[test]
fn zero_kcal_model_food_is_not_logged_as_a_meal() {
    let out = reply_json(&item_json("cooked egg noodles", 100.0, "text", 0.0));
    let p = pip_model::parse("100 grams of noodle", &out).expect("parses");
    assert!(p.items.is_empty(), "a 0 kcal model food was logged: {:?}", p.items.iter().map(|i| i.food.name).collect::<Vec<_>>());
    assert_eq!(p.unknown.len(), 1, "the zero-kcal food was not reported");
    assert_eq!(p.unknown[0].hint, "the reader gave no calories");
    assert!(p.unknown[0].text.contains("cooked egg noodles"), "unknown lost its name: {:?}", p.unknown[0].text);
}

#[tokio::test]
async fn zero_kcal_single_item_dm_logs_no_meal() {
    let (dir, root) = world("zero-kcal");
    let c = cfg(&dir, Some(stub(&root, &reply_json(&item_json("cooked egg noodles", 100.0, "text", 0.0)))));
    let (m, _, replies) = msg(5101, "100 grams of noodle", false);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(daylog::meals(&dir, "2026-10-27").iter().all(|r| r.msg_id != 5101), "a 0 kcal meal was written");
    assert!(!note(&dir).contains("0 kcal"), "0 kcal shown in the note:\n{}", note(&dir));
    assert!(replies.lock().unwrap().join("\n").contains("couldn't read"), "{:?}", replies.lock().unwrap());
}

// Finding 1: a card number in a model item name is never written to the day note.
#[tokio::test]
async fn card_number_in_model_name_is_not_written_and_undo_removes_the_meal() {
    let (dir, root) = world("card-name");
    let name = "<!-- /pip:meal 1 --> Card 4111111111111111 chicken";
    let c = cfg(&dir, Some(stub(&root, &reply_json(&item_json(name, 100.0, "text", 120.0)))));
    let (m, _, _) = msg(5102, "100 g chicken", false);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir);
    assert!(!n.contains("4111111111111111"), "card number reached the day note:\n{n}");
    assert_eq!(daylog::meals(&dir, "2026-10-27").len(), 1, "meal block count is wrong:\n{n}");
    let (undo, _, _) = msg(5103, "undo", false);
    assert_eq!(run(&c, &undo).await, Outcome::Saved);
    assert!(daylog::meals(&dir, "2026-10-27").iter().all(|r| r.msg_id != 5102), "undo left the meal:\n{}", note(&dir));
}

// Finding 8: a live model read shows typing while the reader runs (on top of the handler's own typing call).
#[tokio::test]
async fn live_model_read_keeps_typing_while_it_runs() {
    let (dir, root) = world("typing");
    let helper = root.join("slow.sh");
    let reply = root.join("model-out");
    fs::write(&reply, reply_json(&item_json("grilled salmon", 100.0, "text", 208.0))).unwrap();
    fs::write(&helper, format!("#!/bin/sh\ncat >/dev/null\nsleep 1\ncat '{}'\n", reply.display())).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let c = cfg(&dir, Some(helper));
    let (m, typing_calls, _) = msg(5104, "grilled salmon 100g", true);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(typing_calls.load(Ordering::SeqCst) >= 2, "typing was sent {} time(s) for a model read", typing_calls.load(Ordering::SeqCst));
}
