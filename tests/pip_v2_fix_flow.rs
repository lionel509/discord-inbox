//! F3: the command gate and the fix race, through `pip::handle` with a fake `Inbound`. Scratch dirs live under the OS temp dir; no real notes are touched.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use discord_inbox::rich::Rich;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAIN: u64 = 100000000000000001;
const OTHER: u64 = 100000000000000002;
const DAY: &str = "2026-10-26";
const T19: &str = "2026-10-26T19:30:00-04:00";

struct Msg {
    id: u64,
    author: u64,
    text: String,
    yield_typing: bool,
    replies: Mutex<Vec<String>>,
}

impl Msg {
    fn new(id: u64, text: &str) -> Msg {
        Msg { id, author: MAIN, text: text.into(), yield_typing: false, replies: Mutex::new(Vec::new()) }
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
        T19.into()
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
    async fn typing(&self) {
        if self.yield_typing {
            tokio::task::yield_now().await;
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pipfix-{}-{name}", std::process::id()));
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

fn live_meals(nut: &Path) -> Vec<u64> {
    daylog::meals(nut, DAY).iter().map(|m| m.msg_id).collect()
}

fn note(nut: &Path) -> String {
    fs::read_to_string(daylog::note_path(nut, DAY)).unwrap_or_default()
}

// A sender off the allowlist gets no reply and no change, for every command word.
#[tokio::test]
async fn non_allowlisted_commands_are_ignored() {
    let nut = scratch("gate");
    assert_eq!(run(&nut, &Msg::new(1, "salmon 100g")).await, Outcome::Saved);
    for (i, t) in ["undo", "today", "fix salmon 120g"].iter().enumerate() {
        let mut m = Msg::new(10 + i as u64, t);
        m.author = OTHER;
        assert_eq!(run(&nut, &m).await, Outcome::Ignored, "{t}");
        assert!(m.replies.lock().unwrap().is_empty(), "{t} got a reply");
    }
    assert_eq!(live_meals(&nut), vec![1], "note:\n{}", note(&nut));
}

// The same `fix` delivered twice at once (live handler plus catch-up): one applies, one is Skipped.
#[tokio::test]
async fn fix_delivered_twice_leaves_one_meal_at_120g() {
    let nut = scratch("fix-twice");
    assert_eq!(run(&nut, &Msg::new(1, "salmon 100g")).await, Outcome::Saved);
    let mut a = Msg::new(2, "fix salmon 120g");
    a.yield_typing = true;
    let mut b = Msg::new(2, "fix salmon 120g");
    b.yield_typing = true;
    let c = cfg(&nut);
    let al = allow();
    let (x, y) = tokio::join!(pip::handle(&al, &c, &a), pip::handle(&al, &c, &b));
    let mut outs = vec![x, y];
    outs.sort_by_key(|o| *o == Outcome::Skipped);
    assert_eq!(outs, vec![Outcome::Saved, Outcome::Skipped], "note:\n{}", note(&nut));
    assert_eq!(live_meals(&nut), vec![2], "note:\n{}", note(&nut));
    let n = note(&nut);
    assert!(n.contains("120 g salmon"), "fix should log 120 g:\n{n}");
    assert!(!n.contains("100 g salmon") || n.contains("<!-- pip:undone 1 -->"), "meal 1 should be undone:\n{n}");
}
