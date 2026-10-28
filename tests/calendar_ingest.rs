// School-calendar DM flow (#95): "done with <task>" checks off an event, answered before any chat call. No match falls through to chat.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome};
use discord_inbox::vaults::Vaults;
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

// Every test in this binary points the calendar env at the stub and a scratch calendars file. Tests run one at a time so the stub log is theirs alone.
fn guard() -> MutexGuard<'static, ()> {
    INIT.call_once(|| {
        let tmp = std::env::temp_dir().join(format!("di-calendar-ing-{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        let calendars = tmp.join("calendars");
        fs::write(&calendars, "# school\nMATH 101 - Work\nPHYS 102 - Exams\n").unwrap();
        std::env::set_var("DISCORD_INBOX_CALENDARS_FILE", &calendars);
        std::env::set_var("DISCORD_INBOX_OSASCRIPT", PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/calendar_stub.sh"));
        std::env::set_var("DISCORD_INBOX_CALENDAR_DB", PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calendar.sqlitedb"));
        std::env::set_var("DISCORD_INBOX_CALENDAR_LOG", tmp.join("stub.log"));
        let _ = fs::remove_file(tmp.join("stub.log"));
    });
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn log_lines() -> Vec<String> {
    let p = std::env::var_os("DISCORD_INBOX_CALENDAR_LOG").map(PathBuf::from).unwrap();
    fs::read_to_string(p).unwrap_or_default().lines().map(String::from).collect()
}

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

// Returns (inbox, vault root). Folders are made-up test fixtures.
fn setup(name: &str) -> (PathBuf, PathBuf) {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("di-calendar-ing-{}-{name}-{n}", std::process::id()));
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
        "2026-10-02T15:00:00Z".to_string()
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

#[tokio::test]
async fn done_dm_checks_off_and_replies() {
    let _g = guard();
    let (inbox, root) = setup("done");
    let before = log_lines().len();
    let (out, replies) = say(&inbox, &root, Seq::new(&["#1"]), 1, "done with math hw 3", &[]).await;
    assert!(matches!(out, Outcome::Saved), "{out:?}");
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(replies[0].contains("Checked off ✅ HW 3 due"), "{replies:?}");
    assert!(log_lines().split_off(before).iter().any(|l| l == "check\tMATH 101 - Work\tu-math-hw3"));
}

#[tokio::test]
async fn no_match_falls_through_to_chat() {
    let _g = guard();
    let (inbox, root) = setup("nomatch");
    let before = log_lines().len();
    let (out, replies) = say(&inbox, &root, Seq::new(&["NONE", "Got it, saved."]), 2, "finished the seltzer run", &[]).await;
    assert!(matches!(out, Outcome::Saved), "{out:?}");
    assert!(replies.last().is_some_and(|r| r.contains("Got it")), "{replies:?}");
    assert!(!log_lines().split_off(before).iter().any(|l| l.starts_with("check")));
}

#[tokio::test]
async fn attachment_skips_calendar() {
    let _g = guard();
    let (inbox, root) = setup("attach");
    let before = log_lines().len();
    let (out, replies) = say(&inbox, &root, Seq::new(&["Got it, saved."]), 3, "done with math hw 3", &["notes.txt"]).await;
    assert!(matches!(out, Outcome::Saved), "{out:?}");
    assert!(replies.last().is_some_and(|r| r.contains("Got it")), "{replies:?}");
    assert!(log_lines().split_off(before).is_empty(), "the stub was called for an attachment message");
}
