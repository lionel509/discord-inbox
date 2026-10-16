// Breaker round 2 for PR #43 (head e0785e6). Throwaway: not part of the PR.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::dump::{self, Incoming};
use discord_inbox::ingest::{catch_up_with, ingest_with, Filer, History, Inbound, PAGE};
use discord_inbox::vaults::{self, Vaults, RUNNING, TARGETS};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;
static N: AtomicUsize = AtomicUsize::new(0);

type Replies = Arc<Mutex<Vec<(u64, String)>>>;

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("di-breaker-r2-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("Inbox")
}

fn vault_root(inbox: &Path) -> PathBuf {
    let root = inbox.parent().unwrap().to_path_buf();
    for t in TARGETS.iter() {
        fs::create_dir_all(root.join(t.folder)).unwrap();
    }
    root
}

#[derive(Clone)]
struct Spec {
    id: u64,
    text: String,
    received: String,
    atts: Vec<String>,
}

fn spec(id: u64, text: &str, received: &str) -> Spec {
    Spec { id, text: text.to_string(), received: received.to_string(), atts: Vec::new() }
}

struct Msg {
    s: Spec,
    replies: Replies,
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        MAIN
    }
    fn msg_id(&self) -> u64 {
        self.s.id
    }
    fn channel_id(&self) -> u64 {
        CH
    }
    fn received(&self) -> String {
        self.s.received.clone()
    }
    fn content(&self) -> String {
        self.s.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.s.atts.clone()
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(b"receipt bytes".to_vec())
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
        self.replies.lock().unwrap().push((self.s.id, text.to_string()));
        Ok(())
    }
}

fn msg(specs: &Spec, replies: &Replies) -> Msg {
    Msg { s: specs.clone(), replies: replies.clone() }
}

fn replies_for(r: &Replies, id: u64) -> Vec<String> {
    r.lock().unwrap().iter().filter(|(i, _)| *i == id).map(|(_, t)| t.clone()).collect()
}

struct Hist {
    specs: Vec<Spec>,
    replies: Replies,
}

impl History for Hist {
    type Msg = Msg;

    async fn dm_channel(&self, user_id: u64) -> Result<u64, String> {
        if user_id == MAIN {
            Ok(CH)
        } else {
            Err("no dm channel".into())
        }
    }

    async fn page(&self, channel: u64, after: Option<u64>) -> Result<Vec<Msg>, String> {
        if channel != CH {
            return Ok(Vec::new());
        }
        let mut v: Vec<Spec> = self.specs.iter().filter(|s| after.is_none_or(|a| s.id > a)).cloned().collect();
        v.sort_by_key(|s| s.id);
        match after {
            None => {
                let start = v.len().saturating_sub(PAGE);
                v = v.split_off(start);
            }
            Some(_) => v.truncate(PAGE),
        }
        v.reverse();
        Ok(v.iter().map(|s| Msg { s: s.clone(), replies: self.replies.clone() }).collect())
    }

    async fn fetch(&self, _channel: u64, id: u64) -> Result<Msg, String> {
        self.specs
            .iter()
            .find(|s| s.id == id)
            .map(|s| Msg { s: s.clone(), replies: self.replies.clone() })
            .ok_or_else(|| "unknown message".to_string())
    }
}

/// Every call fails with the same text.
struct Fail(String);

impl Chat for Fail {
    fn ask(&self, _s: &str, _u: &str) -> Result<String, String> {
        Err(self.0.clone())
    }
}

/// Every call fails with a text that changes per call, the way a serde JSON error does (column number).
struct Varying(AtomicUsize);

impl Chat for Varying {
    fn ask(&self, _s: &str, _u: &str) -> Result<String, String> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Err(format!("bad json: expected value at line 1 column {}", n + 1))
    }
}

/// Answers from a script, one per call.
struct Script(Mutex<VecDeque<Result<String, String>>>);

impl Chat for Script {
    fn ask(&self, _s: &str, _u: &str) -> Result<String, String> {
        self.0.lock().unwrap().pop_front().unwrap_or_else(|| Err("script empty".into()))
    }
}

fn script(items: Vec<Result<String, String>>) -> Arc<Script> {
    Arc::new(Script(Mutex::new(items.into())))
}

fn inbox_files(inbox: &Path, id: u64) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(inbox)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.contains(&id.to_string()))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn files_in(dir: &Path) -> Vec<String> {
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default()
}

/// A catch-up notice fires once; a live DM after it, same failure, must still be told.
/// Expected: the live DM gets a notice. Actual (see comment): it gets none.
#[tokio::test]
async fn breaker_r2_live_failure_after_catch_up_is_still_told() {
    let inbox = scratch("live-after-catchup");
    let replies: Replies = Default::default();
    let f = Filer { chat: Arc::new(Fail("http 401".into())), vaults: Vaults::new(vault_root(&inbox)), last_notice: Default::default(), window: Default::default() };
    let old = spec(8001, "old dm", "2026-10-13T10:00:00-04:00");
    let hist = Hist { specs: vec![old], replies: replies.clone() };
    catch_up_with(&allow(), &inbox, &hist, &f).await;
    assert_eq!(replies_for(&replies, 8001).len(), 1, "setup: catch-up should notice once");

    let live = spec(8002, "eggs from costco", "2026-10-14T10:00:00-04:00");
    *f.last_notice.lock().unwrap() = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(601));
    ingest_with(&allow(), &inbox, &msg(&live, &replies), &f).await;
    let got = replies_for(&replies, 8002);
    assert!(!got.is_empty(), "live DM after catch-up got no reply while chat is failing");
}

/// The throttle compares the exact error text. A malformed-JSON error carries a position that changes
/// per response, so every old DM is a "new" failure. Expected: at most one notice for the run.
#[tokio::test]
async fn breaker_r2_throttle_survives_error_text_that_changes_per_message() {
    let inbox = scratch("varying");
    let replies: Replies = Default::default();
    let f = Filer { chat: Arc::new(Varying(AtomicUsize::new(0))), vaults: Vaults::new(vault_root(&inbox)), last_notice: Default::default(), window: Default::default() };
    let specs: Vec<Spec> = (1..=100u64).map(|i| spec(9000 + i, &format!("old dm {i}"), "2026-10-13T10:00:00-04:00")).collect();
    let hist = Hist { specs, replies: replies.clone() };
    catch_up_with(&allow(), &inbox, &hist, &f).await;
    let n = replies.lock().unwrap().len();
    assert!(n <= 1, "100 old DMs with a per-message malformed-JSON error sent {n} notices (expected at most 1)");
}

/// The issue says a question "should be answered, not filed". The guard only covers running notes, so a
/// question the model files under a normal folder is saved as a note. Expected: no note filed, answered.
#[tokio::test]
async fn breaker_r2_question_filed_to_normal_folder_is_answered_not_filed() {
    let inbox = scratch("q-normal");
    let root = vault_root(&inbox);
    let replies: Replies = Default::default();
    let f = Filer {
        chat: script(vec![
            Ok("FOLDER: Citadel/Ideas\nTITLE: Passport location\nTAGS: admin\nREPLY:\nFiled.\n".into()),
            Ok("Your passport is in the top drawer, according to your notes.".into()),
        ]),
        vaults: Vaults::new(root.clone()),
        last_notice: Default::default(),
        window: Default::default(),
    };
    let m = spec(9201, "where did I put the passport?", "2026-10-14T23:06:00-04:00");
    ingest_with(&allow(), &inbox, &msg(&m, &replies), &f).await;
    let filed = files_in(&root.join("Citadel/Ideas"));
    assert!(filed.is_empty(), "a question was filed as a note: {filed:?}");
    let got = replies_for(&replies, 9201);
    assert!(got.first().is_some_and(|r| !r.starts_with("Answered") && r.starts_with("Your passport")), "{got:?}");
}

/// Markdown bullet before the label (`- FOLDER: Shopping List`) is not stripped, so the list is not routed.
#[test]
fn breaker_r2_bulleted_folder_label_still_routes() {
    let inbox = scratch("bullet");
    let v = Vaults::new(vault_root(&inbox));
    let d = vaults::parse_decision(&v, "- FOLDER: Shopping List\nTITLE: Costco run\nREPLY: Added.");
    assert!(d.running.is_some() || d.target.is_some(), "routing lost: answer={} reply={:?}", d.answer, d.reply);
}

/// A space before the colon (`FOLDER : Shopping List`) does not match the `folder:` label.
#[test]
fn breaker_r2_space_before_colon_still_routes() {
    let inbox = scratch("spacecolon");
    let v = Vaults::new(vault_root(&inbox));
    let d = vaults::parse_decision(&v, "FOLDER : Shopping List\nTITLE: Costco run\nREPLY: Added.");
    assert!(d.running.is_some() || d.target.is_some(), "routing lost: answer={} reply={:?}", d.answer, d.reply);
}

/// Underscore emphasis around the label (`__FOLDER:__`) is not stripped by the label match.
#[test]
fn breaker_r2_underscore_emphasis_label_still_routes() {
    let inbox = scratch("underscore");
    let v = Vaults::new(vault_root(&inbox));
    let d = vaults::parse_decision(&v, "__FOLDER:__ Shopping List\n__TITLE:__ Costco run\n__REPLY:__ Added.");
    assert!(d.running.is_some() || d.target.is_some(), "routing lost: answer={} reply={:?}", d.answer, d.reply);
}
