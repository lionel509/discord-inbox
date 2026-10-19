//! Offline DM transcript for the typed commands (#92). Each DM runs through the real `ingest_with`, in
//! a temp folder layout (never `~/Documents`), with a scripted chat. The chat's scripted answer is fake;
//! what is real is the window the model is sent: a fact is in it before `/clear`, and not after.
//! `/commands` and `/clear` never reach the model; their replies are printed as the handler sends them.
//!
//! Run: `cargo run --example commands_demo`
use discord_inbox::allowlist;
use discord_inbox::chat::Chat;
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome};
use discord_inbox::vaults::Vaults;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const SENDER: u64 = 100000000000000001;
const CHANNEL: u64 = 7001;
const FIRST_ID: u64 = 1430000000000000000;

struct Msg {
    id: u64,
    text: String,
    received: String,
    sent: Arc<Mutex<Vec<String>>>,
}

impl Inbound for Msg {
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
        self.received.clone()
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
        self.sent.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

/// Answers from a script, one entry per model call, and prints the window the model was sent.
struct Scripted(Mutex<VecDeque<String>>);

impl Chat for Scripted {
    fn ask(&self, _system: &str, user: &str) -> Result<String, String> {
        let next = self.0.lock().unwrap().pop_front().ok_or("script empty")?;
        // The window is the part of the prompt before "Now he says:". Print it so the memory is visible.
        let window = match user.split_once("\n\nNow he says:") {
            Some((head, _)) if head.starts_with("Earlier in this chat") => head.lines().skip(1).collect::<Vec<_>>().join(" | "),
            _ => "(no earlier turns)".to_string(),
        };
        println!("   model sees: {window}");
        Ok(next)
    }
}

fn decision(reply: &str) -> String {
    format!("FOLDER: Inbox\nTITLE: Chat\nTAGS: chat\nREPLY:\n{reply}\n")
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let root: PathBuf = std::env::temp_dir().join(format!("di-commands-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("Inbox")).unwrap();
    let inbox = root.join("Inbox");
    let chat = Scripted(Mutex::new(
        // Filing DM: one call. A question DM: a filing call, then the answer call.
        vec![
            decision("Noted, teal it is."),
            decision("Answering a question about your earlier message."),
            "You told me your favourite colour is teal.".to_string(),
            decision("Answering a question about your earlier message."),
            "I don't have anything from earlier in this chat.".to_string(),
        ]
        .into(),
    ));
    let filer = Filer { chat: Arc::new(chat), vaults: Vaults::new(root.clone()), last_notice: Default::default(), window: Default::default() };
    let allow = allowlist::parse(&format!("{SENDER}\n")).expect("demo allowlist");
    let sent = Arc::new(Mutex::new(Vec::new()));
    // Real clock: the window ages turns against now, so the stamps must be recent.
    let now = chrono::Utc::now();
    let script = [
        "my favourite colour is teal",
        "what did I just tell you?",
        "/clear",
        "what did I just tell you?",
        "/commands",
    ];
    let mut ok = true;
    for (k, text) in script.iter().enumerate() {
        let before = sent.lock().unwrap().len();
        let m = Msg {
            id: FIRST_ID + k as u64,
            text: text.to_string(),
            received: (now + chrono::Duration::seconds(k as i64)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            sent: sent.clone(),
        };
        println!("DM: {text}");
        if ingest_with(&allow, &inbox, &m, &filer).await != Outcome::Saved {
            ok = false;
        }
        for r in sent.lock().unwrap()[before..].iter() {
            println!("Stormy:\n```\n{r}\n```");
        }
    }
    let _ = fs::remove_dir_all(&root);
    println!("{}", if ok { "PASS" } else { "FAIL" });
}
