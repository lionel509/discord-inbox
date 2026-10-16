//! Offline chat-and-filing demo. Three text DMs run through the real `ingest_with` against a temp
//! copy of the folder layout (never `~/Documents`), with a scripted chat. `--live` reruns the same
//! three DMs through MiMo Flash (at most 3 real calls) when the OpenRouter key file exists.
//!
//! Run: `cargo run --release --example chat_demo [-- --live]`
use discord_inbox::allowlist;
use discord_inbox::chat::{self, Chat, OpenRouter};
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome};
use discord_inbox::vaults::{Vaults, TARGETS};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

const SENDER: u64 = 100000000000000001;
const CHANNEL: u64 = 7001;
const FIRST_ID: u64 = 1430000000000000000;
const RECEIVED: &str = "2026-10-13T14:03:22Z";
const DMS: [&str; 3] = [
    "idea: a small robot arm that sorts lego by colour",
    "recipe: scallion pancakes, flour water salt, fry till crisp",
    "my knee hurts when I go up stairs",
];

struct DemoMsg {
    id: u64,
    text: String,
}

impl Inbound for DemoMsg {
    fn author_id(&self) -> u64 {
        SENDER
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        CHANNEL
    }
    fn received(&self) -> String {
        RECEIVED.to_string()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        Vec::new()
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Err("no attachments".into())
    }
    async fn react(&self, _e: &str) -> Result<(), String> {
        Ok(())
    }
    async fn unreact(&self, _e: &str) -> Result<(), String> {
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        Vec::new()
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        println!("-- reply --\n{text}");
        Ok(())
    }
}

struct Scripted(Mutex<VecDeque<String>>);

impl Chat for Scripted {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        self.0.lock().unwrap().pop_front().ok_or("script empty".into())
    }
}

fn layout() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("di-chat-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("Inbox")).unwrap();
    for t in TARGETS.iter() {
        fs::create_dir_all(root.join(t.folder)).unwrap();
    }
    fs::create_dir_all(root.join("Private")).unwrap();
    fs::write(root.join("Private/note.md"), "private\n").unwrap();
    (root.join("Inbox"), root)
}

fn tree(dir: &Path, base: &Path) {
    let mut items: Vec<PathBuf> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    items.sort();
    for p in items {
        println!("  {}", p.strip_prefix(base).unwrap().display());
        if p.is_dir() {
            tree(&p, base);
        }
    }
}

async fn run(label: &str, chat: Arc<dyn Chat>) -> bool {
    println!("=== {label} ===");
    let (inbox, root) = layout();
    let filer = Filer {
        chat,
        vaults: Vaults::new(root.clone()),
        last_notice: Default::default(),
        window: Default::default(),
    };
    let allow = allowlist::parse(&format!("{SENDER}\n")).expect("demo allowlist");
    let mut ok = true;
    for (k, text) in DMS.iter().enumerate() {
        println!("DM: {text}");
        let m = DemoMsg {
            id: FIRST_ID + k as u64,
            text: text.to_string(),
        };
        if ingest_with(&allow, &inbox, &m, &filer).await != Outcome::Saved {
            ok = false;
        }
    }
    println!("-- tree --");
    tree(&root, &root);
    println!("-- .filed --");
    print!("{}", fs::read_to_string(inbox.join(".filed")).unwrap_or_default());
    let private = fs::read_dir(root.join("Private")).unwrap().count();
    let pass = ok && private == 1;
    println!("Private files: {private}");
    let _ = fs::remove_dir_all(&root);
    pass
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let script = [
        "FOLDER: Citadel/Ideas\nTITLE: Lego sorting robot arm\nTAGS: robotics, lego\nREPLY:\nNice idea, filed with your project ideas.",
        "FOLDER: Fidelity/Want to Make\nTITLE: Scallion pancakes\nTAGS: recipe\nREPLY:\nSaved the recipe.",
        "FOLDER: Inbox\nTITLE: Knee pain\nTAGS: health\nREPLY:\nSorry about the knee. Kept it in Inbox, and it may be worth a doctor's look if it lingers.",
    ];
    let scripted = Arc::new(Scripted(Mutex::new(
        script.iter().map(|s| s.to_string()).collect(),
    )));
    let mut pass = run("scripted chat", scripted).await;
    if std::env::args().any(|a| a == "--live") {
        match OpenRouter::from_key_file(&chat::key_path()) {
            Some(c) => pass &= run("live MiMo Flash", Arc::new(c)).await,
            None => println!("no key, live skipped"),
        }
    }
    println!("{}", if pass { "PASS" } else { "FAIL" });
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
