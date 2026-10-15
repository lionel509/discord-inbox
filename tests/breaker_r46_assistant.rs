// Breaker round 1 for PR #46 (head bbf4b98). Throwaway: not part of the PR.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::dump;
use discord_inbox::ingest::{ingest_with, Filer, Inbound};
use discord_inbox::vaults::{Vaults, LOOKUP_CHARS, TARGETS};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;
static N: AtomicUsize = AtomicUsize::new(0);

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

/// Fresh vault root with every target folder. Returns the root; the Inbox is `root/Inbox`.
fn new_root(name: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("di-breaker-r46-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for t in TARGETS.iter() {
        fs::create_dir_all(root.join(t.folder)).unwrap();
    }
    root
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// A live (or catch-up) DM from the owner.
struct Msg {
    id: u64,
    text: String,
    live: bool,
    hang_typing: bool,
    replies: Arc<Mutex<Vec<String>>>,
    typing: Arc<Mutex<usize>>,
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
        "2026-10-15T09:00:00-04:00".to_string()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        Vec::new()
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Err("no attachments".to_string())
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
    fn live(&self) -> bool {
        self.live
    }
    async fn typing(&self) {
        *self.typing.lock().unwrap() += 1;
        if self.hang_typing {
            std::future::pending::<()>().await;
        }
    }
}

/// Scripted chat: answers pop in call order. `panic_first` panics on the first call.
struct Script {
    answers: Mutex<VecDeque<Result<String, String>>>,
    prompts: Mutex<Vec<String>>,
    panic_first: bool,
}

impl Script {
    fn new(answers: Vec<Result<String, String>>) -> Arc<Script> {
        Arc::new(Script { answers: Mutex::new(answers.into()), prompts: Mutex::new(Vec::new()), panic_first: false })
    }
}

impl Chat for Script {
    fn ask(&self, _system: &str, user: &str) -> Result<String, String> {
        let first = {
            let mut p = self.prompts.lock().unwrap();
            p.push(user.to_string());
            p.len() == 1
        };
        if first && self.panic_first {
            panic!("model exploded");
        }
        self.answers.lock().unwrap().pop_front().unwrap_or_else(|| Err("script empty".to_string()))
    }
}

fn filer(root: &Path, chat: Arc<Script>) -> Filer {
    Filer { chat, vaults: Vaults::new(root.to_path_buf()), last_notice: Default::default() }
}

fn filing(folder: &str, reply: &str) -> Result<String, String> {
    Ok(format!("FOLDER: {folder}\nTITLE: Receipt note\nTAGS: receipt\nREPLY:\n{reply}\n"))
}

fn answering(reply: &str) -> Result<String, String> {
    Ok(format!("FOLDER: Answer\nTITLE: q\nTAGS: \nREPLY:\n{reply}\n"))
}

fn msg(id: u64, text: &str, live: bool, hang_typing: bool) -> (Msg, Arc<Mutex<Vec<String>>>, Arc<Mutex<usize>>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let typing = Arc::new(Mutex::new(0usize));
    let m = Msg { id, text: text.to_string(), live, hang_typing, replies: replies.clone(), typing: typing.clone() };
    (m, replies, typing)
}

fn inbox_of(root: &Path) -> PathBuf {
    root.join("Inbox")
}

// 1. A filed confirmation must not start with an internal status label (issue #45 test line).
#[tokio::test]
async fn breaker_filed_reply_starts_with_no_status_label() {
    let root = new_root("filed");
    let chat = Script::new(vec![filing("Berkshire", "Saved your receipt.")]);
    let f = filer(&root, chat);
    let (m, replies, _) = msg(1427000000000000001, "receipt for the store run", true, false);
    let _ = ingest_with(&allow(), &inbox_of(&root), &m, &f).await;
    let got = replies.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "expected one reply, got {got:?}");
    assert!(!got[0].starts_with("Filed:"), "reply starts with status label: {:?}", got[0]);
    assert!(got[0].ends_with("(Saved to Berkshire.)"), "no saved line: {:?}", got[0]);
    assert!(!got[0].contains(".md"), "reply names a file: {:?}", got[0]);
}

// 2. Chat-failure notice must not start with "Kept in Inbox".
#[tokio::test]
async fn breaker_chat_failure_notice_starts_with_no_status_label() {
    let root = new_root("chatfail");
    let chat = Script::new(vec![Err("boom 500".to_string())]);
    let f = filer(&root, chat);
    let (m, replies, _) = msg(1427000000000000002, "something to save", true, false);
    let _ = ingest_with(&allow(), &inbox_of(&root), &m, &f).await;
    let got = replies.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "expected one reply, got {got:?}");
    assert!(!got[0].starts_with("Kept in Inbox"), "reply starts with status label: {:?}", got[0]);
    assert!(!got[0].contains("boom 500"), "reply leaks the raw error: {:?}", got[0]);
}

// 3. An empty model answer on the Answer route must not send an empty Discord message.
#[tokio::test]
async fn breaker_empty_answer_sends_nonempty_reply() {
    let root = new_root("emptyans");
    let chat = Script::new(vec![answering("On it."), Ok(String::new())]);
    let f = filer(&root, chat);
    let (m, replies, _) = msg(1427000000000000003, "how much does willam owe me?", true, false);
    let _ = ingest_with(&allow(), &inbox_of(&root), &m, &f).await;
    let got = replies.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "expected one reply, got {got:?}");
    assert!(!got[0].trim().is_empty(), "reply is empty: {:?}", got[0]);
}

// 4. A typing call that never returns must not hold the reply hostage.
#[tokio::test]
async fn breaker_hung_typing_call_does_not_block_reply() {
    let root = new_root("hangtype");
    let chat = Script::new(vec![filing("Berkshire", "Saved.")]);
    let f = filer(&root, chat);
    let (m, replies, _) = msg(1427000000000000004, "receipt", true, true);
    let r = tokio::time::timeout(Duration::from_secs(10), ingest_with(&allow(), &inbox_of(&root), &m, &f)).await;
    assert!(r.is_ok(), "ingest hung on a typing call that never returned");
    assert_eq!(replies.lock().unwrap().len(), 1);
}

// 5. A huge IOU ledger must still be read when the running note is already at its 6 000-char tail.
#[test]
fn breaker_big_ledger_is_dropped_when_running_note_is_full() {
    let root = new_root("bigledger");
    write(&root, "State Street/Cash Flow/Spending/Shopping List.md", &"a".repeat(6000));
    write(&root, "State Street/Assets/Receivables/Who Owes Me.md", &format!("MARK-LEDGER owes 40\n{}", "y".repeat(6000)));
    let v = Vaults::new(root.clone());
    let notes = dump::lookup_notes_with(
        &inbox_of(&root),
        &v.allowed_dirs(),
        &v.running_paths(),
        &v.ledger().into_iter().collect::<Vec<_>>(),
        LOOKUP_CHARS,
    );
    let labels: Vec<&str> = notes.iter().map(|(l, _)| l.as_str()).collect();
    assert!(labels.iter().any(|l| l.ends_with("Who Owes Me.md")), "ledger missing from lookup: {labels:?}");
}

// 6. A question with the ledger in its note set must not send the ledger on a filing route.
#[tokio::test]
async fn breaker_ledger_not_sent_on_filing_route() {
    let root = new_root("ledgerfile");
    write(&root, "State Street/Assets/Receivables/Who Owes Me.md", "MARK-LEDGER owes 40\n");
    let chat = Script::new(vec![filing("Berkshire", "Saved.")]);
    let f = filer(&root, chat.clone());
    let (m, _, _) = msg(1427000000000000006, "bought stickers for the store", true, false);
    let _ = ingest_with(&allow(), &inbox_of(&root), &m, &f).await;
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts.iter().all(|p| !p.contains("MARK-LEDGER")), "ledger reached a filing prompt");
}

// 7. The ledger path must not resolve through a symlinked folder to a file outside the vault.
#[test]
fn breaker_ledger_symlinked_folder_is_refused() {
    let root = new_root("symlink");
    let outside = new_root("outside");
    write(&outside, "Assets/Receivables/Who Owes Me.md", "MARK-OUTSIDE owes 40\n");
    let _ = fs::remove_dir_all(root.join("State Street"));
    fs::create_dir_all(root.join("State Street")).unwrap();
    std::os::unix::fs::symlink(outside.join("Assets"), root.join("State Street/Assets")).unwrap();
    let v = Vaults::new(root.clone());
    assert!(v.ledger().is_none(), "ledger resolved outside the vault: {:?}", v.ledger());
}

// 8. A ledger of 3-byte characters past the 6 000-char cut must not panic and must stay in budget.
#[test]
fn breaker_huge_multibyte_ledger_is_cut_on_char_boundary() {
    let root = new_root("utf8");
    write(&root, "State Street/Assets/Receivables/Who Owes Me.md", &"€".repeat(10_000));
    let v = Vaults::new(root.clone());
    let notes = dump::lookup_notes_with(&inbox_of(&root), &v.allowed_dirs(), &v.running_paths(), &v.ledger().into_iter().collect::<Vec<_>>(), LOOKUP_CHARS);
    let ledger = notes.iter().find(|(l, _)| l.ends_with("Who Owes Me.md")).expect("ledger missing");
    assert_eq!(ledger.1.chars().count(), 4000);
}

// 9. A panic in the model call must not leave the reply or the ingest hanging.
#[tokio::test]
async fn breaker_model_panic_does_not_hang_ingest() {
    let root = new_root("panic");
    let chat = Arc::new(Script {
        answers: Mutex::new(VecDeque::from(vec![filing("Berkshire", "Saved.")])),
        prompts: Mutex::new(Vec::new()),
        panic_first: true,
    });
    let f = filer(&root, chat);
    let (m, _replies, typing) = msg(1427000000000000009, "receipt", true, false);
    let r = tokio::time::timeout(Duration::from_secs(10), ingest_with(&allow(), &inbox_of(&root), &m, &f)).await;
    assert!(r.is_ok(), "ingest hung after a model panic");
    let n = *typing.lock().unwrap();
    assert!(n >= 1 && n <= 2, "typing count {n}");
}
