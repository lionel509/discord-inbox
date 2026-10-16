// Breaker tests for PR #43 (ingest/chat flow). Throwaway: not part of the PR.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::ingest::{catch_up_with, ingest_with, Filer, History, Inbound, Outcome, PAGE};
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
    let dir = std::env::temp_dir().join(format!("di-breaker-{}-{name}-{n}", std::process::id()));
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
        self.replies.lock().unwrap().push((self.s.id, text.to_string()));
        Ok(())
    }
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
        let mut v: Vec<Spec> = self
            .specs
            .iter()
            .filter(|s| after.is_none_or(|a| s.id > a))
            .cloned()
            .collect();
        v.sort_by_key(|s| s.id);
        match after {
            None => {
                let start = v.len().saturating_sub(PAGE);
                v = v.split_off(start);
            }
            Some(_) => v.truncate(PAGE),
        }
        v.reverse();
        Ok(v.into_iter().map(|s| Msg { s, replies: self.replies.clone() }).collect())
    }

    async fn fetch(&self, _channel: u64, id: u64) -> Result<Msg, String> {
        self.specs
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .map(|s| Msg { s, replies: self.replies.clone() })
            .ok_or_else(|| "unknown message".to_string())
    }
}

struct Canned {
    answers: Mutex<VecDeque<Result<String, String>>>,
}

impl Chat for Canned {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("script empty".into()))
    }
}

fn canned(answers: Vec<Result<String, String>>) -> Arc<Canned> {
    Arc::new(Canned { answers: Mutex::new(answers.into()) })
}

fn filer_with(inbox: &Path, chat: Arc<Canned>) -> Filer {
    Filer { chat, vaults: Vaults::new(vault_root(inbox)), last_notice: Default::default(), window: Default::default() }
}

fn spec(id: u64, text: &str, received: &str) -> Spec {
    Spec { id, text: text.to_string(), received: received.to_string() }
}

/// Inbox entries whose file name carries message id `id`.
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

/// Breaks: a chat failure (chat off here) during a catch-up of 100 old, never-seen DMs.
/// Expected: the user hears about the failure at most once for the whole run, not 100 times.
#[tokio::test]
async fn breaker_chat_off_catch_up_does_not_notice_every_old_dm() {
    let inbox = scratch("spam");
    let replies: Replies = Default::default();
    let specs: Vec<Spec> = (1..=100u64)
        .map(|i| spec(5000 + i, &format!("old dm {i}"), "2026-10-13T10:00:00Z"))
        .collect();
    let hist = Hist { specs, replies: replies.clone() };
    catch_up_with(&allow(), &inbox, &hist, &Filer::off()).await;
    let n = replies.lock().unwrap().len();
    assert!(n <= 1, "catch-up of 100 old DMs with chat off sent {n} replies (expected at most 1)");
}

/// Breaks: the stamp changed from UTC `Z` to local `±HH:MM`, and the note name is built from it.
/// A DM saved by the old build (name `..._100000_6001.md`) is not "saved" under the new name
/// (`..._060000_6001.md`), so the first start after deploy saves it a second time.
/// Expected: one Inbox entry for the message.
#[tokio::test]
async fn breaker_pre_deploy_inbox_note_is_not_saved_twice_after_stamp_change() {
    let inbox = scratch("upgrade");
    let replies: Replies = Default::default();
    let old = Msg {
        s: spec(6001, "milk for the week", "2026-10-13T10:00:00Z"),
        replies: replies.clone(),
    };
    let first = ingest_with(&allow(), &inbox, &old, &Filer::off()).await;
    assert_eq!(first, Outcome::Saved);
    assert_eq!(inbox_files(&inbox, 6001).len(), 1, "setup: {:?}", inbox_files(&inbox, 6001));

    // Same DM after the deploy: the same message, stamped in local time (EDT).
    let hist = Hist {
        specs: vec![spec(6001, "milk for the week", "2026-10-13T06:00:00-04:00")],
        replies: replies.clone(),
    };
    catch_up_with(&allow(), &inbox, &hist, &Filer::off()).await;
    let after = inbox_files(&inbox, 6001);
    assert_eq!(after.len(), 1, "one DM, {} Inbox entries after restart: {after:?}", after.len());
}

/// Breaks: a model folder label in bold markdown (`**FOLDER:** Shopping List`) is not read as a label,
/// so a list message is neither appended nor filed and the raw text goes out as the reply.
#[test]
fn breaker_bolded_folder_label_still_routes_to_running_note() {
    let inbox = scratch("bold");
    let v = Vaults::new(vault_root(&inbox));
    let d = vaults::parse_decision(
        &v,
        "**FOLDER:** Shopping List\n**TITLE:** Costco run\n**REPLY:** Added to the list.",
    );
    assert!(
        d.running.is_some() || d.target.is_some(),
        "routing lost: answer={} reply={:?}",
        d.answer,
        d.reply
    );
}

/// Breaks: a trailing full stop after the key (`FOLDER: Shopping List.`) is not stripped by `clean_key`,
/// so the key does not match and the message is not routed.
#[test]
fn breaker_trailing_period_after_folder_key_still_routes() {
    let inbox = scratch("period");
    let v = Vaults::new(vault_root(&inbox));
    let d = vaults::parse_decision(&v, "FOLDER: Shopping List.\nTITLE: Costco run\nREPLY: Added.");
    assert!(
        d.running.is_some() || d.target.is_some(),
        "routing lost: answer={} reply={:?}",
        d.answer,
        d.reply
    );
}

/// Breaks: a question ("whats on my costco like list?") that the model files under the running list is
/// appended as a shopping item, and the reply says "Filed ... (appended)". Nothing on the code side
/// checks for a question. Expected: the question is not appended to the list.
#[tokio::test]
async fn breaker_question_routed_to_running_list_is_not_appended_as_item() {
    let inbox = scratch("question");
    let root = inbox.parent().unwrap().to_path_buf();
    let replies: Replies = Default::default();
    let list = root.join(RUNNING[0].path);
    fs::create_dir_all(list.parent().unwrap()).unwrap();
    fs::write(&list, "# Shopping List\n\n- [ ] eggs\n\n**Hub:** [[Spending]]\n").unwrap();

    let chat = canned(vec![Ok(
        "FOLDER: Shopping List\nTITLE: Costco list lookup\nTAGS: groceries\nREPLY:\nYou have eggs on the list.\n"
            .to_string(),
    )]);
    let f = filer_with(&inbox, chat);
    let msg = Msg {
        s: spec(7001, "whats on my costco like list?", "2026-10-14T23:06:00Z"),
        replies: replies.clone(),
    };
    let out = ingest_with(&allow(), &inbox, &msg, &f).await;
    let text = fs::read_to_string(&list).unwrap();
    let reply = replies.lock().unwrap().first().map(|r| r.1.clone()).unwrap_or_default();
    assert!(
        !text.contains("whats on my costco"),
        "question appended as a shopping item (outcome {out:?}); list now:\n{text}\nreply: {reply:?}"
    );
}
