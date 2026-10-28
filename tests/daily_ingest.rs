// Daily check-in reply (#135): a text DM while the night marker reads `sent` is answered by daily::answer; anything else files as before. The model is a scripted fake and the calendar is the stub.
use chrono::{Local, Utc};
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Slot};
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome};
use discord_inbox::vaults::Vaults;
use discord_inbox::when;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once};

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;
static N: AtomicUsize = AtomicUsize::new(0);
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

fn guard() -> MutexGuard<'static, ()> {
    INIT.call_once(|| {
        let tmp = std::env::temp_dir().join(format!("di-daily-ing-{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        let calendars = tmp.join("calendars");
        fs::write(&calendars, "MATH 101 - Work\n").unwrap();
        std::env::set_var("DISCORD_INBOX_CALENDARS_FILE", &calendars);
        std::env::set_var("DISCORD_INBOX_OSASCRIPT", PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/calendar_stub.sh"));
        std::env::set_var("DISCORD_INBOX_CALENDAR_LOG", tmp.join("stub.log"));
    });
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

// Returns (inbox, vault root). Folders are made-up test fixtures.
fn setup(name: &str) -> (PathBuf, PathBuf) {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("di-daily-ing-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    (inbox, root)
}

/// Answers from a script, in order. Asked once more than scripted, it panics: the test caught an extra model call.
struct Seq(Mutex<VecDeque<String>>);
impl Seq {
    fn new(replies: &[&str]) -> Arc<Seq> {
        Arc::new(Seq(Mutex::new(replies.iter().map(|s| s.to_string()).collect())))
    }
}
impl Chat for Seq {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        match self.0.lock().unwrap().pop_front() {
            Some(text) => Ok(text),
            None => panic!("the model was asked more times than scripted"),
        }
    }
}

fn filer(root: &Path, chat: Arc<Seq>) -> Filer {
    Filer { chat, vaults: Vaults::new(root.to_path_buf()), last_notice: Default::default(), window: Default::default() }
}

struct Msg {
    id: u64,
    text: String,
    attach: Vec<String>,
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
        // A live reply arrives after the night DM was staged, so it is the reply and not a replay.
        when::local_stamp(Utc::now())
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.attach.clone()
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

async fn say(inbox: &Path, root: &Path, chat: Arc<Seq>, id: u64, text: &str, attach: &[&str]) -> (Outcome, Vec<String>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id, text: text.to_string(), attach: attach.iter().map(|s| s.to_string()).collect(), replies: replies.clone() };
    let out = ingest_with(&allow(), inbox, &m, &filer(root, chat)).await;
    let all = replies.lock().unwrap().clone();
    (out, all)
}

fn today() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

fn lab_todo(root: &Path, day: &str) {
    let dir = root.join(discord_inbox::todo::DIR);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{day}.md")), "---\ntags:\n  - todo\n---\n- finish lab report\n").unwrap();
}

fn marker(inbox: &Path, slot: Slot, day: &str) -> Option<String> {
    fs::read_to_string(daily::marker_path(inbox, slot, day)).ok().map(|s| s.trim().to_string())
}

fn has_note(inbox: &Path) -> bool {
    fs::read_dir(inbox).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().ends_with(".md"))
}

#[tokio::test]
async fn night_reply_is_a_checkin() {
    let _g = guard();
    let (inbox, root) = setup("checkin");
    let day = today();
    lab_todo(&root, &day);
    daily::stage(&inbox, Slot::Night, &day).unwrap();
    let chat = Seq::new(&[r#"{"checkin":true,"done":[1],"tomorrow":[]}"#]);
    let (out, replies) = say(&inbox, &root, chat, 1, "finished the lab report", &[]).await;
    assert!(matches!(out, Outcome::Saved), "{out:?}");
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(replies[0].contains("finish lab report"), "{replies:?}");
    assert_eq!(marker(&inbox, Slot::Night, &day).as_deref(), Some("answered"));
    assert!(!has_note(&inbox), "a check-in reply must not file a note");
}

#[tokio::test]
async fn night_message_that_is_not_a_checkin_files_as_before() {
    let _g = guard();
    let (inbox, root) = setup("notcheckin");
    let day = today();
    lab_todo(&root, &day);
    daily::stage(&inbox, Slot::Night, &day).unwrap();
    let chat = Seq::new(&[r#"{"checkin":false,"done":[],"tomorrow":[]}"#, "ok", "ok"]);
    let (_out, replies) = say(&inbox, &root, chat, 2, "what time is the library open", &[]).await;
    assert_eq!(marker(&inbox, Slot::Night, &day).as_deref(), Some("sent"));
    assert!(!replies.iter().any(|r| r.contains("Checked off")), "{replies:?}");
}

#[tokio::test]
async fn night_photo_is_never_a_checkin() {
    let _g = guard();
    let (inbox, root) = setup("photo");
    let day = today();
    lab_todo(&root, &day);
    daily::stage(&inbox, Slot::Night, &day).unwrap();
    let chat = Seq::new(&["x", "x", "x"]);
    let (_out, _replies) = say(&inbox, &root, chat, 3, "here is my lab", &["lab.png"]).await;
    assert_eq!(marker(&inbox, Slot::Night, &day).as_deref(), Some("sent"));
}

#[tokio::test]
async fn no_night_marker_means_normal_filing() {
    let _g = guard();
    let (inbox, root) = setup("nomarker");
    let day = today();
    lab_todo(&root, &day);
    let chat = Seq::new(&["x", "x", "x"]);
    let (_out, replies) = say(&inbox, &root, chat, 4, "here is my lab", &[]).await;
    assert!(marker(&inbox, Slot::Night, &day).is_none());
    assert!(!replies.iter().any(|r| r.contains("Checked off")), "{replies:?}");
}
