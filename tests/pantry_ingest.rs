// Pantry DM flow (#50): a restock or "out of" DM is answered from the pantry, the model is never asked.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome};
use discord_inbox::vaults::Vaults;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;
static N: AtomicUsize = AtomicUsize::new(0);

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

// Returns (inbox, vault root). Folders are made-up test fixtures.
fn setup(name: &str) -> (PathBuf, PathBuf) {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("di-pantry-ing-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for f in ["Fidelity/Pantry", "State Street/Cash Flow/Spending/Splits"] {
        fs::create_dir_all(root.join(f)).unwrap();
    }
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    (inbox, root)
}

struct Never;
impl Chat for Never {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        panic!("the model must not be asked for a pantry command");
    }
}

fn filer(root: &Path) -> Filer {
    Filer { chat: Arc::new(Never), vaults: Vaults::new(root.to_path_buf()), last_notice: Default::default() }
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
        Err("no attachments".into())
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

async fn say(inbox: &Path, root: &Path, id: u64, text: &str) -> (Outcome, String) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id, text: text.to_string(), replies: replies.clone() };
    let out = ingest_with(&allow(), inbox, &m, &filer(root)).await;
    let last = replies.lock().unwrap().last().cloned().unwrap_or_default();
    (out, last)
}

#[tokio::test]
async fn restock_dm_updates_pantry_without_calling_the_model() {
    let (inbox, root) = setup("restock");
    fs::write(
        root.join("State Street/Cash Flow/Spending/Splits/Costco Split — 2026-10-02.md"),
        "# Costco Split\n\n| Item | Qty | Who | Price |\n|---|---|---|---|\n| KS ATL SALMON — salmon | 1 | Lionel ½ · Ana ½ | 20 |\n",
    )
    .unwrap();
    let (out, reply) = say(&inbox, &root, 1, "restock 2026-10-02").await;
    assert!(matches!(out, Outcome::Saved), "{out:?}");
    assert!(reply.contains("+salmon"), "{reply}");
    let note = fs::read_to_string(root.join("Fidelity/Pantry/On Hand.md")).unwrap();
    assert!(note.contains("| salmon |"), "{note}");
}

#[tokio::test]
async fn out_of_dm_drops_row() {
    let (inbox, root) = setup("outof");
    fs::write(root.join("Fidelity/Pantry/On Hand.md"), "# On Hand\n\n| Item | Qty | Store | Bought | Days |\n|---|---|---|---|---|\n| cod |  | Costco | 2026-10-02 | 13 |\n").unwrap();
    let (out, reply) = say(&inbox, &root, 2, "out of cod").await;
    assert!(matches!(out, Outcome::Saved), "{out:?}");
    assert!(reply.starts_with("Dropped cod"), "{reply}");
    let note = fs::read_to_string(root.join("Fidelity/Pantry/On Hand.md")).unwrap();
    assert!(!note.contains("| cod |"), "{note}");
    let kept = fs::read_dir(&inbox).unwrap().filter_map(|e| fs::read_to_string(e.unwrap().path()).ok()).any(|t| t.contains("out of cod"));
    assert!(kept, "the raw command message stays in the Inbox");
}
