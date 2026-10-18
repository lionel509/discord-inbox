//! Offline chat-and-filing demo. Text DMs run through the real `ingest_with` against a temp copy of
//! the folder layout (never `~/Documents`), with a scripted chat. The first run is three filing DMs.
//! The second section runs the #73 question cases: a screenshot with a question (the OCR text the
//! model sees is printed, redacted), a question with no "?" that gets a lookup promise (the promise
//! is never sent; the answer is), and the same DM when the lookup fails (the promise is not repeated).
//! `--live` reruns the first section through MiMo Pro (at most 3 real calls) when the key file exists.
//!
//! The screenshot case reads text with the Apple Vision helper that build.rs compiles (macOS only).
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
/// A made-up receipt photo with a made-up card number; the card number must be masked before the model sees it.
const CARD: &[u8] = include_bytes!("assets/card.png");
/// The promise the chat model makes on the question path (#73). Its text must never reach Discord.
const PROMISE: &str = "Let me dig through your notes for that, give me a sec 🐾";

/// One DM: its text and its attachments (name, bytes).
type Dm = (&'static str, Vec<(&'static str, Vec<u8>)>);

struct DemoMsg {
    id: u64,
    text: String,
    files: Vec<(&'static str, Vec<u8>)>,
    sent: Arc<Mutex<Vec<String>>>,
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
        self.files.iter().map(|(n, _)| n.to_string()).collect()
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.files.get(i).map(|(_, b)| b.clone()).ok_or("no attachment".into())
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
        self.sent.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

/// Answers from a script, one entry per model call. With `show`, prints what the model was sent and what it said.
struct Scripted {
    replies: Mutex<VecDeque<Result<String, String>>>,
    show: bool,
}

impl Chat for Scripted {
    fn ask(&self, _system: &str, user: &str) -> Result<String, String> {
        let next = self.replies.lock().unwrap().pop_front().unwrap_or(Err("script empty".into()));
        if self.show {
            println!("-- model sees --\n{user}");
            match &next {
                Ok(t) => println!("-- model says --\n{t}"),
                Err(e) => println!("-- model fails --\n{e}"),
            }
        }
        next
    }
}

fn scripted(replies: Vec<Result<String, String>>, show: bool) -> Arc<dyn Chat> {
    Arc::new(Scripted { replies: Mutex::new(replies.into()), show })
}

fn decision(folder: &str, title: &str, tags: &str, reply: &str) -> Result<String, String> {
    Ok(format!("FOLDER: {folder}\nTITLE: {title}\nTAGS: {tags}\nREPLY:\n{reply}\n"))
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

/// Runs the DMs in order. `forbid` is text that must not appear in any reply sent.
async fn run(label: &str, chat: Arc<dyn Chat>, dms: &[Dm], forbid: Option<&str>) -> bool {
    println!("=== {label} ===");
    let (inbox, root) = layout();
    let filer = Filer {
        chat,
        vaults: Vaults::new(root.clone()),
        last_notice: Default::default(),
        window: Default::default(),
    };
    let allow = allowlist::parse(&format!("{SENDER}\n")).expect("demo allowlist");
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut ok = true;
    for (k, (text, files)) in dms.iter().enumerate() {
        println!("DM: {text}");
        if !files.is_empty() {
            let names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
            println!("attached: {}", names.join(", "));
        }
        let m = DemoMsg {
            id: FIRST_ID + k as u64,
            text: text.to_string(),
            files: files.clone(),
            sent: sent.clone(),
        };
        if ingest_with(&allow, &inbox, &m, &filer).await != Outcome::Saved {
            ok = false;
        }
        if let Some(bad) = forbid {
            if sent.lock().unwrap().iter().any(|r| r.contains(bad)) {
                println!("FAIL: a reply contains {bad:?}");
                ok = false;
            }
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
    let filing: Vec<Dm> = DMS.iter().map(|t| (*t, Vec::new())).collect();
    let script = vec![
        decision("Citadel/Ideas", "Lego sorting robot arm", "robotics, lego", "Nice idea, filed with your project ideas."),
        decision("Fidelity/Want to Make", "Scallion pancakes", "recipe", "Saved the recipe."),
        decision("Inbox", "Knee pain", "health", "Sorry about the knee. Kept it in Inbox, and it may be worth a doctor's look if it lingers."),
    ];
    let mut pass = run("scripted chat", scripted(script, false), &filing, None).await;
    if std::env::args().any(|a| a == "--live") {
        match OpenRouter::from_key_file(&chat::key_path()) {
            Some(c) => pass &= run("live MiMo Pro", Arc::new(c), &filing, None).await,
            None => println!("no key, live skipped"),
        }
    }

    // #73: a screenshot with a question. The model sees the OCR text after redaction, then answers from it.
    let screenshot: Vec<Dm> = vec![("what does this say", vec![("card.png", CARD.to_vec())])];
    pass &= run(
        "question: screenshot",
        scripted(
            vec![
                decision("Answer", "Card receipt", "shopping", "Checking the picture."),
                Ok("It says TRADER JOES, a total of 12.40. The card number is masked, so I can't read it.".to_string()),
            ],
            true,
        ),
        &screenshot,
        None,
    )
    .await;

    // #73: a question with no "?" that gets a lookup promise. The promise is never sent; the answer is.
    let promise: Vec<Dm> = vec![("where did i put my passport", Vec::new())];
    pass &= run(
        "question: promise, no question mark",
        scripted(
            vec![
                decision("Inbox", "Passport spot", "misc", PROMISE),
                Ok("Your passport is in the blue folder on the shelf.".to_string()),
            ],
            true,
        ),
        &promise,
        Some("give me a sec"),
    )
    .await;

    // The same DM when the lookup fails: the reply keeps the words but not the promise, and is sent once.
    pass &= run(
        "question: promise, then lookup fails",
        scripted(
            vec![decision("Inbox", "Passport spot", "misc", PROMISE), Err("http 500".to_string())],
            true,
        ),
        &promise,
        Some("give me a sec"),
    )
    .await;

    println!("{}", if pass { "PASS" } else { "FAIL" });
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
