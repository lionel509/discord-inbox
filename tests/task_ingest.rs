// Stormy's small-task route (#84): a money text goes to the sandboxed helper, not the chat model, and its reply is the DM's answer.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome};
use discord_inbox::vaults::Vaults;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7002;
static N: AtomicUsize = AtomicUsize::new(0);
static ENV: Mutex<()> = Mutex::new(());

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

// Returns (inbox, vault root, scratch dir for the fake). Folders are made-up test fixtures.
fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("di-task-ing-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let root = base.join("vaults");
    fs::create_dir_all(root.join("State Street")).unwrap();
    let inbox = base.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    let out = base.join("out");
    fs::create_dir_all(&out).unwrap();
    (inbox, root, out)
}

/// A fake `claude`: records its stdin and that it ran, then answers.
fn fake(out: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = out.join("claude-fake");
    let body = format!("#!/bin/sh\ncat > '{o}/stdin'\necho ran > '{o}/ran'\nprintf 'Booked it 🐾\\n'\n", o = out.display());
    fs::write(&p, body).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

struct Never;
impl Chat for Never {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        panic!("the chat model must not answer a money text");
    }
}

struct Canned;
impl Chat for Canned {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Ok("FOLDER: Chat\nREPLY:\nok".to_string())
    }
}

fn filer(root: &Path, chat: Arc<dyn Chat>) -> Filer {
    Filer { chat, vaults: Vaults::new(root.to_path_buf()), last_notice: Default::default(), window: Default::default() }
}

struct Msg {
    id: u64,
    text: String,
    attachments: Vec<String>,
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
        "2026-10-18T15:00:00Z".to_string()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.attachments.clone()
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(b"a pasted receipt".to_vec())
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

async fn say(inbox: &Path, f: &Filer, id: u64, text: &str, attachments: &[&str]) -> (Outcome, Vec<String>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id, text: text.to_string(), attachments: attachments.iter().map(|s| s.to_string()).collect(), replies: replies.clone() };
    let out = ingest_with(&allow(), inbox, &m, f).await;
    let all = replies.lock().unwrap().clone();
    (out, all)
}

fn set_env(claude: &Path, tasks: Option<&str>) {
    std::env::set_var("DISCORD_INBOX_CLAUDE", claude);
    std::env::set_var("DISCORD_INBOX_INSIGHT", "");
    match tasks {
        Some(v) => std::env::set_var("DISCORD_INBOX_TASKS", v),
        None => std::env::remove_var("DISCORD_INBOX_TASKS"),
    }
}

fn clear_env() {
    std::env::remove_var("DISCORD_INBOX_CLAUDE");
    std::env::remove_var("DISCORD_INBOX_TASKS");
}

#[tokio::test]
async fn zelle_text_is_booked_by_the_helper() {
    let _g = ENV.lock().unwrap_or_else(|p| p.into_inner());
    let (inbox, root, out) = setup("zelle");
    set_env(&fake(&out), None);
    let f = filer(&root, Arc::new(Never));
    let (o, replies) = say(&inbox, &f, 1, "Chase | Zelle(R): PACO M NHAN sent you $25.40 & it's ready now.", &[]).await;
    clear_env();

    assert!(matches!(o, Outcome::Saved), "{o:?}");
    assert_eq!(replies, vec!["Booked it 🐾".to_string()]);
    let prompt = fs::read_to_string(out.join("stdin")).unwrap();
    assert!(prompt.contains("PACO") && prompt.contains("25.40"), "{prompt}");
}

#[tokio::test]
async fn money_out_text_goes_to_the_helper() {
    let _g = ENV.lock().unwrap_or_else(|p| p.into_inner());
    let (inbox, root, out) = setup("out");
    set_env(&fake(&out), None);
    let f = filer(&root, Arc::new(Never));
    let text = "im sending paco back 50 cus he split the 3d printer fialment with me and he used none of it so far";
    let (o, replies) = say(&inbox, &f, 2, text, &[]).await;
    clear_env();

    assert!(matches!(o, Outcome::Saved), "{o:?}");
    assert_eq!(replies, vec!["Booked it 🐾".to_string()]);
    assert!(fs::read_to_string(out.join("stdin")).unwrap().contains("50"));
}

#[tokio::test]
async fn tasks_off_keeps_the_old_path() {
    let _g = ENV.lock().unwrap_or_else(|p| p.into_inner());
    let (inbox, root, out) = setup("off");
    set_env(&fake(&out), Some("off"));
    let f = filer(&root, Arc::new(Canned));
    let (_o, replies) = say(&inbox, &f, 3, "Chase | Zelle(R): PACO M NHAN sent you $25.40 & it's ready now.", &[]).await;
    clear_env();

    assert!(!out.join("ran").exists(), "the helper must not run when tasks are off");
    assert!(!replies.iter().any(|r| r.contains("Booked it")), "{replies:?}");
}

#[tokio::test]
async fn attachment_never_spawns_the_helper() {
    let _g = ENV.lock().unwrap_or_else(|p| p.into_inner());
    let (inbox, root, out) = setup("attach");
    set_env(&fake(&out), None);
    let f = filer(&root, Arc::new(Canned));
    let (_o, replies) = say(&inbox, &f, 4, "Chase | Zelle(R): PACO M NHAN sent you $25.40", &["receipt.txt"]).await;
    clear_env();

    assert!(!out.join("ran").exists(), "a DM with an attachment stays on the filing path");
    assert!(!replies.iter().any(|r| r.contains("Booked it")), "{replies:?}");
}
