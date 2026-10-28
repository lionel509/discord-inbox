// Breaker finding for PR #138 (head 64e75ad): a text DM inside the night check-in window is filed to Inbox when the model is down. Harness copied from tests/daily_ingest.rs.
use chrono::Local;
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Slot};
use discord_inbox::ingest::{ingest_with, Filer, Inbound};
use discord_inbox::vaults::Vaults;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;

struct Down;
impl Chat for Down {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Err("model down".to_string())
    }
}

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

struct Msg {
    id: u64,
    text: String,
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
        CH
    }
    fn received(&self) -> String {
        "2026-10-02T15:00:00Z".to_string()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        Vec::new()
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(b"hi".to_vec())
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
}

fn has_note(inbox: &Path) -> bool {
    fs::read_dir(inbox).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().ends_with(".md"))
}

// B9. The file header says "anything else files as before". With the night marker at `sent` and the model erroring, a plain note DM goes to daily::answer, returns the check-in saved text, and ingest returns Saved without writing Inbox.
#[tokio::test]
async fn b9_night_window_note_is_filed_when_model_is_down() {
    let root: PathBuf = std::env::temp_dir().join(format!("breaker138-b9-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    daily::stage(&inbox, Slot::Night, &today).unwrap();
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id: 900, text: "remind me to call the landlord about the lease".to_string(), replies: replies.clone() };
    let filer = Filer {
        chat: Arc::new(Down),
        vaults: Vaults::new(root.clone()),
        last_notice: Default::default(),
        window: Default::default(),
    };
    let out = ingest_with(&allow(), &inbox, &m, &filer).await;
    // Filing also returns Saved, so the outcome cannot tell the paths apart: check the reply text and the Inbox.
    assert!(replies.lock().unwrap().iter().all(|r| !r.contains("Logged your reply")), "a non-check-in note was swallowed by the check-in path: {:?}", replies.lock().unwrap());
    assert!(has_note(&inbox), "the note was not filed to Inbox; replies sent: {:?} outcome: {out:?}", replies.lock().unwrap());
    assert_eq!(fs::read_to_string(daily::marker_path(&inbox, Slot::Night, &today)).unwrap().trim(), "sent");
}

