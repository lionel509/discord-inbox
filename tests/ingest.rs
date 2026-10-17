//! Behaviour of `ingest` and `catch_up` against fakes. Fake IDs only.
use discord_inbox::allowlist;
use discord_inbox::allowlist::Allowlist;
use discord_inbox::dump::{
    self, add_retry, in_retry, note_is_complete, read_cursor, retry_entries, sha256_hex,
    write_cursor,
};
use discord_inbox::ingest::{catch_up, ingest, History, Inbound, Outcome, PAGE};
use discord_inbox::chat::Chat;
use discord_inbox::ingest::{catch_up_with, ingest_with, Filer};
use discord_inbox::vaults::{Vaults, TARGETS};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, OnceLock};

const MAIN: u64 = 100000000000000001;
const ALT: u64 = 100000000000000002;
const STRANGER: u64 = 999999999999999999;
const MAIN_CH: u64 = 7001;
const ALT_CH: u64 = 7002;
const STRANGER_CH: u64 = 7009;
const RECEIVED: &str = "2026-10-13T14:03:22Z";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n{ALT}\n")).unwrap()
}

/// Stands in for the Vision helper: reads the image on stdin, answers only for receipts.
/// Set once for the whole test binary, so no test reaches the real helper.
fn ocr_stub() -> &'static Path {
    static STUB: OnceLock<PathBuf> = OnceLock::new();
    STUB.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("di-ocr-stub-{}", std::process::id()));
        let body = r#"#!/bin/sh
in=$(cat)
case "$in" in *card*) printf 'TRADER JOES\nTOTAL  12.34\nVISA 4111 1111 1111 1111\n' ;; *down*) printf 'MODELDOWN\nTOTAL  1.00\n' ;; *receipt*) printf 'TRADER JOES\nTOTAL  12.34\nVISA 1234\n' ;; *) exit 1 ;; esac
"#;
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("DISCORD_INBOX_OCR", &path);
        path
    })
}

/// Stands in for the local model: reads the prompt on stdin. Set once, with a budget note, for the whole binary.
fn insight_stub() {
    static STUB: OnceLock<()> = OnceLock::new();
    STUB.get_or_init(|| {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("di-insight-stub-{pid}"));
        let body = r#"#!/bin/sh
in=$(cat)
case "$in" in
  *MODELDOWN*) exit 1 ;;
  *TRADER*) printf '%s\n' "receipt: yes" "merchant: Trader Joe's" "total: 12.34" "category: groceries" ;;
  *) printf 'receipt: no\n' ;;
esac
"#;
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let budget = std::env::temp_dir().join(format!("di-budget-{pid}"));
        let _ = fs::remove_dir_all(&budget);
        fs::create_dir_all(&budget).unwrap();
        fs::write(budget.join("2026-10.md"), "budget: $800\n- 2026-10-02 Groceries $45.10\n| 2026-10-05 | Lunch | $12.90 |\n").unwrap();
        std::env::set_var("DISCORD_INBOX_INSIGHT", &path);
        std::env::set_var("DISCORD_INBOX_BUDGET", budget.join("{month}.md"));
    });
}

/// A fresh scratch dir; returns its `Inbox` path, which is NOT created.
fn scratch(name: &str) -> PathBuf {
    ocr_stub();
    insight_stub();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("di-ingest-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("Inbox")
}

fn att(inbox: &Path, id: u64, i: usize, name: &str) -> PathBuf {
    inbox.join(format!("2026-10-13_140322_{id}_{i}_{name}"))
}

fn note(inbox: &Path, id: u64) -> PathBuf {
    dump::note_path(inbox, RECEIVED, id)
}

#[derive(Clone)]
struct Spec {
    author: u64,
    channel: u64,
    id: u64,
    text: String,
    files: Vec<(String, Vec<u8>)>,
}

fn spec(author: u64, channel: u64, id: u64, files: &[(&str, &[u8])]) -> Spec {
    Spec {
        author,
        channel,
        id,
        text: "hi".to_string(),
        files: files
            .iter()
            .map(|(n, b)| (n.to_string(), b.to_vec()))
            .collect(),
    }
}

struct World {
    inbox: PathBuf,
    cdn_down: AtomicBool,
    react_fails: AtomicBool,
    unreact_fails: AtomicBool,
    reply_fails: AtomicBool,
    /// msg id -> (downloads, reacts)
    calls: Mutex<HashMap<u64, (usize, usize)>>,
    note_at_react: Mutex<HashMap<u64, bool>>,
    /// (msg id, text) for every reply attempt, failed or not.
    replies: Mutex<Vec<(u64, String)>>,
    /// msg id -> the bot's reactions now on that message (the Discord-side state).
    marks: Mutex<HashMap<u64, Vec<String>>>,
    /// Every above-the-line "typing" call, by msg id (#45).
    typing: Mutex<Vec<u64>>,
    /// Every react/unreact attempt in order: (msg id, "react"|"unreact", emoji).
    ops: Mutex<Vec<(u64, &'static str, String)>>,
}

impl World {
    fn new(inbox: &Path) -> Arc<World> {
        Arc::new(World {
            inbox: inbox.to_path_buf(),
            cdn_down: AtomicBool::new(false),
            react_fails: AtomicBool::new(false),
            unreact_fails: AtomicBool::new(false),
            reply_fails: AtomicBool::new(false),
            calls: Mutex::new(HashMap::new()),
            note_at_react: Mutex::new(HashMap::new()),
            replies: Mutex::new(Vec::new()),
            marks: Mutex::new(HashMap::new()),
            typing: Mutex::new(Vec::new()),
            ops: Mutex::new(Vec::new()),
        })
    }
    fn typing_count(&self, id: u64) -> usize {
        self.typing.lock().unwrap().iter().filter(|m| **m == id).count()
    }
    fn replies_for(&self, id: u64) -> Vec<String> {
        self.replies
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| *m == id)
            .map(|(_, t)| t.clone())
            .collect()
    }
    fn marks(&self, id: u64) -> Vec<String> {
        self.marks.lock().unwrap().get(&id).cloned().unwrap_or_default()
    }
    fn ops_for(&self, id: u64) -> Vec<(&'static str, String)> {
        self.ops
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _, _)| *m == id)
            .map(|(_, k, e)| (*k, e.clone()))
            .collect()
    }
    fn add_download(&self, id: u64) {
        self.calls.lock().unwrap().entry(id).or_insert((0, 0)).0 += 1;
    }
    fn add_react(&self, id: u64) {
        self.calls.lock().unwrap().entry(id).or_insert((0, 0)).1 += 1;
    }
    fn downloads(&self, id: u64) -> usize {
        self.calls.lock().unwrap().get(&id).map_or(0, |c| c.0)
    }
    fn reacts(&self, id: u64) -> usize {
        self.calls.lock().unwrap().get(&id).map_or(0, |c| c.1)
    }
    fn total_downloads(&self) -> usize {
        self.calls.lock().unwrap().values().map(|c| c.0).sum()
    }
    fn total_reacts(&self) -> usize {
        self.calls.lock().unwrap().values().map(|c| c.1).sum()
    }
}

struct FakeMsg {
    spec: Spec,
    w: Arc<World>,
    /// false for a message that came back from history (catch-up), true for a live DM.
    live: bool,
}

impl Inbound for FakeMsg {
    fn author_id(&self) -> u64 {
        self.spec.author
    }
    fn msg_id(&self) -> u64 {
        self.spec.id
    }
    fn channel_id(&self) -> u64 {
        self.spec.channel
    }
    fn received(&self) -> String {
        RECEIVED.to_string()
    }
    fn content(&self) -> String {
        self.spec.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.spec.files.iter().map(|f| f.0.clone()).collect()
    }
    fn live(&self) -> bool {
        self.live
    }
    async fn typing(&self) {
        self.w.typing.lock().unwrap().push(self.spec.id);
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.w.add_download(self.spec.id);
        // A real download awaits the network: yield here so two calls on one message can interleave.
        tokio::task::yield_now().await;
        let (name, bytes) = &self.spec.files[i];
        if self.w.cdn_down.load(Ordering::SeqCst) && name.starts_with("bad") {
            return Err("cdn 503".to_string());
        }
        Ok(bytes.clone())
    }
    async fn react(&self, emoji: &str) -> Result<(), String> {
        self.w.add_react(self.spec.id);
        self.w
            .ops
            .lock()
            .unwrap()
            .push((self.spec.id, "react", emoji.to_string()));
        if emoji == "✅" {
            let at = note(&self.w.inbox, self.spec.id).is_file();
            self.w
                .note_at_react
                .lock()
                .unwrap()
                .insert(self.spec.id, at);
        }
        if self.w.react_fails.load(Ordering::SeqCst) {
            return Err("react 500".to_string());
        }
        let mut marks = self.w.marks.lock().unwrap();
        let v = marks.entry(self.spec.id).or_default();
        if !v.iter().any(|e| e == emoji) {
            v.push(emoji.to_string());
        }
        Ok(())
    }
    async fn unreact(&self, emoji: &str) -> Result<(), String> {
        self.w
            .ops
            .lock()
            .unwrap()
            .push((self.spec.id, "unreact", emoji.to_string()));
        if self.w.unreact_fails.load(Ordering::SeqCst) {
            return Err("unreact 500".to_string());
        }
        if let Some(v) = self.w.marks.lock().unwrap().get_mut(&self.spec.id) {
            v.retain(|e| e != emoji);
        }
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        self.w.marks(self.spec.id)
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        self.w
            .replies
            .lock()
            .unwrap()
            .push((self.spec.id, text.to_string()));
        if self.w.reply_fails.load(Ordering::SeqCst) {
            return Err("reply 500".to_string());
        }
        Ok(())
    }
}

fn msg(w: &Arc<World>, s: &Spec) -> FakeMsg {
    FakeMsg {
        spec: s.clone(),
        w: w.clone(),
        live: true,
    }
}

struct FakeHistory {
    w: Arc<World>,
    msgs: Mutex<Vec<Spec>>,
    /// Counts pages of MAIN_CH only, so the number is the same whatever ALT does.
    page_calls: AtomicUsize,
}

impl FakeHistory {
    fn new(w: &Arc<World>, msgs: Vec<Spec>) -> FakeHistory {
        FakeHistory {
            w: w.clone(),
            msgs: Mutex::new(msgs),
            page_calls: AtomicUsize::new(0),
        }
    }
}

impl History for FakeHistory {
    type Msg = FakeMsg;

    async fn dm_channel(&self, user_id: u64) -> Result<u64, String> {
        match user_id {
            MAIN => Ok(MAIN_CH),
            ALT => Ok(ALT_CH),
            _ => Err("no dm channel".to_string()),
        }
    }

    async fn page(&self, channel: u64, after: Option<u64>) -> Result<Vec<FakeMsg>, String> {
        if channel == MAIN_CH {
            self.page_calls.fetch_add(1, Ordering::SeqCst);
        }
        let mut picked: Vec<Spec> = self
            .msgs
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.channel == channel && after.is_none_or(|a| s.id > a))
            .cloned()
            .collect();
        picked.sort_by_key(|s| s.id);
        match after {
            // The newest PAGE when there is no `after`.
            None => {
                let start = picked.len().saturating_sub(PAGE);
                picked = picked.split_off(start);
            }
            Some(_) => picked.truncate(PAGE),
        }
        // Newest first, the way Discord returns them.
        picked.reverse();
        Ok(picked.into_iter().map(|spec| history_msg(&self.w, &spec)).collect())
    }

    async fn fetch(&self, channel: u64, msg_id: u64) -> Result<FakeMsg, String> {
        let found = self
            .msgs
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == msg_id && s.channel == channel)
            .cloned();
        match found {
            Some(spec) => Ok(history_msg(&self.w, &spec)),
            None => Err("unknown message".to_string()),
        }
    }
}

/// A message as the history calls return it: catch-up, not live.
fn history_msg(w: &Arc<World>, s: &Spec) -> FakeMsg {
    FakeMsg {
        live: false,
        ..msg(w, s)
    }
}

/// Every file name in `inbox` that belongs to message `id`.
fn names_for(inbox: &Path, id: u64) -> Vec<String> {
    let Ok(rd) = fs::read_dir(inbox) else {
        return Vec::new();
    };
    rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(&format!("_{id}.md")) || n.contains(&format!("_{id}_")))
        .collect()
}

#[tokio::test]
async fn stranger_live_dm_triggers_nothing() {
    let inbox = scratch("stranger-live");
    let w = World::new(&inbox);
    let m = FakeMsg {
        spec: spec(STRANGER, STRANGER_CH, 100, &[("bad.jpg", b"b")]),
        w: w.clone(),
        live: true,
    };
    assert_eq!(ingest(&allow(), &inbox, &m).await, Outcome::Ignored);
    assert_eq!(w.downloads(100), 0);
    assert_eq!(w.reacts(100), 0);
    assert!(!inbox.exists());
}

#[tokio::test]
async fn stranger_in_history_never_moves_a_cursor() {
    let inbox = scratch("stranger-history");
    let w = World::new(&inbox);
    let h = FakeHistory::new(&w, vec![spec(STRANGER, MAIN_CH, 500, &[("a.txt", b"x")])]);
    catch_up(&allow(), &inbox, &h).await;
    assert_eq!(read_cursor(&inbox, MAIN_CH), None);
    assert_eq!(w.total_downloads(), 0);
    assert_eq!(w.total_reacts(), 0);
    assert!(!inbox.exists());
}

#[tokio::test]
async fn allowed_dm_is_saved_then_reacted_once() {
    let inbox = scratch("allowed-live");
    let w = World::new(&inbox);
    for (author, channel, id) in [(MAIN, MAIN_CH, 100u64), (ALT, ALT_CH, 200u64)] {
        let s = spec(author, channel, id, &[("a.txt", b"x")]);
        assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
        assert_eq!(w.downloads(id), 1, "id {id}");
        assert_eq!(w.reacts(id), 1, "id {id}");
        assert_eq!(
            w.note_at_react.lock().unwrap().get(&id),
            Some(&true),
            "react must follow the note"
        );
        assert!(note(&inbox, id).is_file());
        assert!(att(&inbox, id, 1, "a.txt").is_file());
        assert!(retry_entries(&inbox).is_empty());
        assert_eq!(
            read_cursor(&inbox, channel),
            None,
            "the live path writes no cursor"
        );
    }
}

#[tokio::test]
async fn failed_download_still_writes_the_note_and_queues_a_retry() {
    let inbox = scratch("failed-download");
    let w = World::new(&inbox);
    w.cdn_down.store(true, Ordering::SeqCst);
    // The failing attachment comes first, so a `break` after the first failure would drop the good one.
    let s = spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b"), ("good.txt", b"g")]);
    assert_eq!(
        ingest(&allow(), &inbox, &msg(&w, &s)).await,
        Outcome::Pending
    );
    assert_eq!(w.downloads(100), 2, "keeps going after a failure");
    assert_eq!(w.marks(100), vec!["⏳"]);
    assert!(
        att(&inbox, 100, 2, "good.txt").is_file(),
        "the file after a failed one is saved"
    );
    assert!(!att(&inbox, 100, 1, "bad.jpg").exists());
    let text = fs::read_to_string(note(&inbox, 100)).unwrap();
    assert!(
        text.contains("  - file: 2026-10-13_140322_100_1_bad.jpg\n    sha256: pending\n"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "  - file: 2026-10-13_140322_100_2_good.txt\n    sha256: {}\n",
            sha256_hex(b"g")
        )),
        "{text}"
    );
    assert_eq!(retry_entries(&inbox), vec![(MAIN_CH, 100)]);
}

#[tokio::test]
async fn next_start_retries_and_completes_the_note() {
    let inbox = scratch("next-start");
    let w = World::new(&inbox);
    let h = FakeHistory::new(
        &w,
        vec![
            spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]),
            spec(MAIN, MAIN_CH, 200, &[]),
        ],
    );
    w.cdn_down.store(true, Ordering::SeqCst);
    catch_up(&allow(), &inbox, &h).await;
    let first = fs::read_to_string(note(&inbox, 100)).unwrap();
    assert!(first.contains("pending"), "{first}");
    assert!(note(&inbox, 200).is_file());
    assert_eq!(retry_entries(&inbox), vec![(MAIN_CH, 100)]);
    assert_eq!(read_cursor(&inbox, MAIN_CH), Some(200));
    assert_eq!(w.marks(100), vec!["⏳"]);
    assert_eq!(w.marks(200), vec!["✅"]);

    w.cdn_down.store(false, Ordering::SeqCst);
    catch_up(&allow(), &inbox, &h).await;
    assert!(retry_entries(&inbox).is_empty());
    let second = fs::read_to_string(note(&inbox, 100)).unwrap();
    assert!(!second.contains("pending"), "{second}");
    assert!(att(&inbox, 100, 1, "bad.jpg").is_file());
    assert_eq!(w.marks(100), vec!["✅"]);
    assert_eq!(w.reacts(200), 1);
    assert_eq!(
        w.total_downloads(),
        2,
        "200 is text-only; 100 was tried twice"
    );
}

#[tokio::test]
async fn cursor_stops_before_a_message_that_was_not_recorded() {
    let inbox = scratch("cursor-stop");
    let w = World::new(&inbox);
    let h = FakeHistory::new(
        &w,
        vec![
            spec(MAIN, MAIN_CH, 100, &[]),
            spec(MAIN, MAIN_CH, 200, &[]),
            spec(MAIN, MAIN_CH, 300, &[]),
        ],
    );
    // A directory where the note goes makes that note's write fail.
    fs::create_dir_all(note(&inbox, 200)).unwrap();
    catch_up(&allow(), &inbox, &h).await;
    assert_eq!(read_cursor(&inbox, MAIN_CH), Some(100));
    assert!(note(&inbox, 100).is_file());
    assert!(note(&inbox, 300).is_file());
    assert_eq!(w.marks(200), vec!["⚠\u{FE0F}"]);
    assert_eq!(w.marks(100), vec!["✅"]);
    assert_eq!(w.marks(300), vec!["✅"]);

    fs::remove_dir(note(&inbox, 200)).unwrap();
    catch_up(&allow(), &inbox, &h).await;
    assert!(note(&inbox, 200).is_file());
    assert_eq!(w.marks(200), vec!["✅"]);
    assert_eq!(
        w.reacts(300),
        1,
        "300 is Skipped on the second pass, not reacted again"
    );
    assert_eq!(read_cursor(&inbox, MAIN_CH), Some(300));
}

#[tokio::test]
async fn failed_react_keeps_the_note_and_no_retry() {
    let inbox = scratch("failed-react");
    let w = World::new(&inbox);
    w.react_fails.store(true, Ordering::SeqCst);
    let s = spec(MAIN, MAIN_CH, 100, &[]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
    assert!(note(&inbox, 100).is_file());
    assert!(retry_entries(&inbox).is_empty());
    assert_eq!(w.reacts(100), 1);
    let downloads = w.total_downloads();
    assert_eq!(
        ingest(&allow(), &inbox, &msg(&w, &s)).await,
        Outcome::Skipped
    );
    assert_eq!(w.reacts(100), 1);
    assert_eq!(w.total_downloads(), downloads);
}

#[tokio::test]
async fn channels_keep_separate_cursors() {
    let inbox = scratch("channels");
    let w = World::new(&inbox);
    let h = FakeHistory::new(
        &w,
        vec![
            spec(MAIN, MAIN_CH, 100, &[]),
            spec(MAIN, MAIN_CH, 300, &[]),
            spec(ALT, ALT_CH, 200, &[]),
        ],
    );
    catch_up(&allow(), &inbox, &h).await;
    assert_eq!(read_cursor(&inbox, MAIN_CH), Some(300));
    assert_eq!(read_cursor(&inbox, ALT_CH), Some(200));

    h.msgs.lock().unwrap().push(spec(ALT, ALT_CH, 250, &[]));
    catch_up(&allow(), &inbox, &h).await;
    assert!(note(&inbox, 250).is_file());
    assert_eq!(w.reacts(250), 1);
}

#[tokio::test]
async fn catch_up_pages_past_one_page() {
    let inbox = scratch("paging");
    let w = World::new(&inbox);
    write_cursor(&inbox, MAIN_CH, 1).unwrap();
    let msgs = (2..=251u64)
        .map(|id| spec(MAIN, MAIN_CH, id, &[]))
        .collect();
    let h = FakeHistory::new(&w, msgs);
    catch_up(&allow(), &inbox, &h).await;
    let notes = fs::read_dir(&inbox)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".md")
        })
        .count();
    assert_eq!(notes, 250);
    assert_eq!(read_cursor(&inbox, MAIN_CH), Some(251));
    assert_eq!(h.page_calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn unfetchable_retry_entry_is_kept() {
    let inbox = scratch("unfetchable");
    let w = World::new(&inbox);
    add_retry(&inbox, MAIN_CH, 999).unwrap();
    let h = FakeHistory::new(&w, vec![]);
    catch_up(&allow(), &inbox, &h).await;
    assert_eq!(retry_entries(&inbox), vec![(MAIN_CH, 999)]);
}

/// Every `.tmp` file left in `inbox`.
fn tmp_files(inbox: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(inbox) else {
        return Vec::new();
    };
    rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[tokio::test]
async fn zero_byte_note_is_repaired_not_skipped() {
    let inbox = scratch("zero-byte");
    let w = World::new(&inbox);
    fs::create_dir_all(&inbox).unwrap();
    fs::write(note(&inbox, 100), b"").unwrap();
    let s = spec(MAIN, MAIN_CH, 100, &[("a.txt", b"x")]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
    let text = fs::read_to_string(note(&inbox, 100)).unwrap();
    assert!(
        text.contains(&sha256_hex(b"x")),
        "empty note left in place: {text:?}"
    );
    assert!(att(&inbox, 100, 1, "a.txt").is_file());
    assert_eq!(w.reacts(100), 1);
    assert!(tmp_files(&inbox).is_empty(), "{:?}", tmp_files(&inbox));
}

#[tokio::test]
async fn failed_note_write_leaves_the_old_note_whole() {
    let inbox = scratch("atomic");
    let w = World::new(&inbox);
    fs::create_dir_all(&inbox).unwrap();
    fs::write(note(&inbox, 100), "old note\n").unwrap();
    add_retry(&inbox, MAIN_CH, 100).unwrap();
    // A directory where the temp file goes makes the note write fail.
    fs::create_dir_all(note(&inbox, 100).with_extension("md.tmp")).unwrap();
    let s = spec(MAIN, MAIN_CH, 100, &[]);
    assert_eq!(
        ingest(&allow(), &inbox, &msg(&w, &s)).await,
        Outcome::Pending // still in .retry, so it is Pending, not Failed
    );
    assert_eq!(fs::read_to_string(note(&inbox, 100)).unwrap(), "old note\n");
}

/// The breaker's race: the live event and the catch-up see one message, and its one download
/// fails for one of them. Each round must end with a complete note or a retry entry.
#[test]
fn live_and_catch_up_on_one_message_never_leave_a_pending_note_unowed() {
    let rounds = 1500;
    let mut bad = 0;
    for round in 0..rounds {
        let inbox = scratch(&format!("race-{round}"));
        let failing = World::new(&inbox);
        failing.cdn_down.store(true, Ordering::SeqCst);
        let ok = World::new(&inbox);
        let s = spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]);
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for w in [failing, ok] {
            let (barrier, s) = (barrier.clone(), s.clone());
            handles.push(std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let inbox = w.inbox.clone();
                barrier.wait();
                rt.block_on(ingest(&allow(), &inbox, &msg(&w, &s)));
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let pending = fs::read_to_string(note(&inbox, 100))
            .map(|t| t.contains("pending"))
            .unwrap_or(false);
        if pending && !in_retry(&inbox, 100) {
            bad += 1;
        }
        let _ = fs::remove_dir_all(inbox.parent().unwrap());
    }
    assert_eq!(
        bad, 0,
        "{bad} of {rounds} rounds ended with a pending note and no retry entry"
    );
}

/// A pending write that reaches the note after a complete one must leave the complete note alone.
#[tokio::test]
async fn pending_write_never_replaces_a_complete_note() {
    let inbox = scratch("pending-after-complete");
    let ok = World::new(&inbox);
    let fail = World::new(&inbox);
    fail.cdn_down.store(true, Ordering::SeqCst);
    let s = spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]);
    assert_eq!(
        ingest(&allow(), &inbox, &msg(&ok, &s)).await,
        Outcome::Saved
    );
    // The failing call's retry record lands after the complete call has cleared it.
    add_retry(&inbox, MAIN_CH, 100).unwrap();
    assert_eq!(
        ingest(&allow(), &inbox, &msg(&fail, &s)).await,
        Outcome::Pending
    );
    let text = fs::read_to_string(note(&inbox, 100)).unwrap();
    assert!(!text.contains("pending"), "{text}");
    assert!(text.contains(&sha256_hex(b"b")), "{text}");
    assert!(att(&inbox, 100, 1, "bad.jpg").is_file());
}

/// A 64-bit LCG, seed 42, so every run draws the same histories.
#[tokio::test]
async fn invariants_hold_for_random_histories() {
    let allow = allow();
    let mut x: u64 = 42;
    let mut draw = |n: u64| -> u64 {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 33) % n
    };

    for round in 0..100u64 {
        let inbox = scratch(&format!("prop-{round}"));
        let w = World::new(&inbox);
        w.cdn_down.store(draw(2) == 0, Ordering::SeqCst);
        let n = 1 + draw(12);
        let mut msgs: Vec<Spec> = Vec::new();
        for k in 1..=n {
            let id = 100 * k + 1;
            let (author, channel) = match draw(3) {
                0 => (MAIN, MAIN_CH),
                1 => (ALT, ALT_CH),
                _ => (STRANGER, STRANGER_CH),
            };
            let mut files: Vec<(String, Vec<u8>)> = Vec::new();
            for i in 1..=draw(3) {
                let name = if draw(2) == 0 {
                    format!("good{i}.txt")
                } else {
                    format!("bad{i}.jpg")
                };
                files.push((name, format!("payload {id} {i}").into_bytes()));
            }
            if author != STRANGER && draw(8) == 0 {
                fs::create_dir_all(note(&inbox, id)).unwrap();
            }
            msgs.push(Spec {
                author,
                channel,
                id,
                text: "hi".to_string(),
                files,
            });
        }

        // Live deliveries: every stranger, and each allowlisted message half the time.
        for s in &msgs {
            if s.author == STRANGER || draw(2) == 0 {
                let _ = ingest(&allow, &inbox, &msg(&w, s)).await;
            }
        }
        let h = FakeHistory::new(&w, msgs.clone());
        catch_up(&allow, &inbox, &h).await;

        // I1: everything at or below a channel's cursor is on disk or queued.
        for ch in [MAIN_CH, ALT_CH] {
            if let Some(c) = read_cursor(&inbox, ch) {
                for s in msgs.iter().filter(|s| s.channel == ch && s.id <= c) {
                    assert!(
                        note(&inbox, s.id).is_file() || in_retry(&inbox, s.id),
                        "round {round}: I1: {} sits at or below cursor {c} with no note and no retry",
                        s.id
                    );
                }
            }
        }
        // I2: a stranger gets no download, no react, no file, no retry and no cursor.
        for s in msgs.iter().filter(|s| s.author == STRANGER) {
            assert_eq!(w.downloads(s.id), 0, "round {round}: I2 download {}", s.id);
            assert_eq!(w.reacts(s.id), 0, "round {round}: I2 react {}", s.id);
            assert!(w.marks(s.id).is_empty(), "round {round}: I2 mark {}", s.id);
            assert!(!in_retry(&inbox, s.id), "round {round}: I2 retry {}", s.id);
            assert!(
                names_for(&inbox, s.id).is_empty(),
                "round {round}: I2 file for {}",
                s.id
            );
        }
        assert!(
            !inbox.join(".cursor-7009").exists(),
            "round {round}: I2 cursor for stranger channel"
        );
        // I3: the mark is the latest outcome, and a message never carries two.
        for s in &msgs {
            let marks = w.marks(s.id);
            let n = note(&inbox, s.id);
            let tag = format!("round {round}: I3 {}", s.id);
            assert!(marks.len() <= 1, "{tag}: marks {marks:?}");
            let one = marks.first().map(String::as_str);
            if one == Some("✅") {
                assert!(n.is_file() && !in_retry(&inbox, s.id), "{tag}: tick");
                let text = fs::read_to_string(&n).unwrap();
                assert!(!text.contains("pending"), "{tag}: pending note ticked");
            }
            if one == Some("⏳") {
                assert!(in_retry(&inbox, s.id), "{tag}: hourglass not queued");
            }
            if one == Some("⚠\u{FE0F}") {
                assert!(!n.is_file(), "{tag}: warning with a note");
            }
            if s.author != STRANGER {
                if n.is_file() && !in_retry(&inbox, s.id) {
                    assert_eq!(marks, vec!["✅"], "{tag}: complete but unticked");
                } else if in_retry(&inbox, s.id) {
                    assert_eq!(marks, vec!["⏳"], "{tag}: queued but no hourglass");
                }
            }
        }
    }
}

#[tokio::test]
async fn receipt_text_is_in_the_note_before_the_tick() {
    let inbox = scratch("ocr-receipt");
    let w = World::new(&inbox);
    let s = spec(
        MAIN,
        MAIN_CH,
        300,
        &[
            ("receipt.jpg", b"receipt bytes"),
            ("notes.txt", b"receipt in text"),
        ],
    );
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
    assert_eq!(w.reacts(300), 1);
    assert_eq!(
        w.note_at_react.lock().unwrap().get(&300),
        Some(&true),
        "react must follow the note"
    );
    let text = fs::read_to_string(note(&inbox, 300)).unwrap();
    assert!(
        text.contains(
            "\n## OCR 2026-10-13_140322_300_1_receipt.jpg\n\n~~~text\nTRADER JOES\nTOTAL  12.34\nVISA 1234\n~~~\n"
        ),
        "{text}"
    );
    assert_eq!(text.matches("## OCR").count(), 1, "{text}");
    assert!(note_is_complete(&note(&inbox, 300)));
    assert!(!in_retry(&inbox, 300));
    let mut names: Vec<String> = fs::read_dir(&inbox)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut want = vec![
        "2026-10-13_140322_300.md".to_string(),
        "2026-10-13_140322_300_1_receipt.jpg".to_string(),
        "2026-10-13_140322_300_2_notes.txt".to_string(),
    ];
    want.sort();
    assert_eq!(names, want);
}

#[tokio::test]
async fn failed_ocr_still_saves_and_ticks() {
    let inbox = scratch("ocr-failed");
    let w = World::new(&inbox);
    let s = spec(MAIN, MAIN_CH, 310, &[("photo.jpg", b"jpeg bytes")]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
    assert_eq!(w.reacts(310), 1);
    assert!(note_is_complete(&note(&inbox, 310)));
    let text = fs::read_to_string(note(&inbox, 310)).unwrap();
    assert!(!text.contains("## OCR"), "{text}");
    assert!(!in_retry(&inbox, 310));
}

#[tokio::test]
async fn pending_download_keeps_ocr_and_retry_semantics() {
    let inbox = scratch("ocr-pending");
    let w = World::new(&inbox);
    w.cdn_down.store(true, Ordering::SeqCst);
    let s = spec(
        MAIN,
        MAIN_CH,
        320,
        &[
            ("receipt.jpg", b"receipt a"),
            ("bad-receipt.jpg", b"receipt b"),
        ],
    );
    assert_eq!(
        ingest(&allow(), &inbox, &msg(&w, &s)).await,
        Outcome::Pending
    );
    assert_eq!(w.marks(320), vec!["⏳"]);
    assert!(in_retry(&inbox, 320));
    let text = fs::read_to_string(note(&inbox, 320)).unwrap();
    assert_eq!(text.matches("## OCR").count(), 1, "{text}");
    assert!(
        text.contains("## OCR 2026-10-13_140322_320_1_receipt.jpg\n"),
        "{text}"
    );
    assert!(text.contains("sha256: pending"), "{text}");

    w.cdn_down.store(false, Ordering::SeqCst);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
    assert_eq!(w.marks(320), vec!["✅"]);
    let text = fs::read_to_string(note(&inbox, 320)).unwrap();
    assert_eq!(text.matches("## OCR").count(), 2, "{text}");
    assert!(!in_retry(&inbox, 320));
}

/// A receipt DM gets exactly one reply, after the tick; re-ingest and catch-up add none.
#[tokio::test]
async fn receipt_dm_replies_once_after_tick() {
    let inbox = scratch("reply-once");
    let w = World::new(&inbox);
    let s = spec(MAIN, MAIN_CH, 100, &[("receipt.png", b"receipt")]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
    assert_eq!(w.reacts(100), 1);
    let replies = w.replies_for(100);
    assert_eq!(replies.len(), 1, "{replies:?}");
    let text = &replies[0];
    assert!(text.contains("Receipt: Trader Joe's · $12.34 · groceries"), "{text}");
    assert!(text.contains("Budget left before this: $742.00 of $800.00 (2026-10)"), "{text}");
    assert!(!text.contains("TRADER JOES"), "{text}");
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Skipped);
    let hist = FakeHistory::new(&w, vec![s.clone()]);
    catch_up(&allow(), &inbox, &hist).await;
    assert_eq!(w.replies_for(100).len(), 1);
}

/// Card digits never reach the reply: only the last four survive.
#[tokio::test]
async fn reply_masks_card_digits() {
    let inbox = scratch("reply-mask");
    let w = World::new(&inbox);
    let s = spec(MAIN, MAIN_CH, 101, &[("receipt.png", b"receipt card")]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
    let replies = w.replies_for(101);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(!replies[0].contains("4111"), "{}", replies[0]);
}

/// Nothing to say means no reply: strangers, a pending receipt. A plain DM with the chat off gets the one failure notice (#40).
#[tokio::test]
async fn no_reply_when_nothing_to_say() {
    let inbox = scratch("reply-none");
    let w = World::new(&inbox);
    let good = spec(MAIN, MAIN_CH, 102, &[("good.txt", b"hello")]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &good)).await, Outcome::Saved);
    let notice = w.replies_for(102);
    assert_eq!(notice.len(), 1, "{notice:?}");
    assert!(notice[0].starts_with("Sorry, my brain's a little foggy right now 😿") && !notice[0].contains("chat off"), "{}", notice[0]);

    let stranger = spec(STRANGER, STRANGER_CH, 103, &[("receipt.png", b"receipt")]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &stranger)).await, Outcome::Ignored);
    assert!(w.replies_for(103).is_empty());
    assert!(w.marks(103).is_empty());

    w.cdn_down.store(true, Ordering::SeqCst);
    let pending = spec(MAIN, MAIN_CH, 104, &[("bad.jpg", b"b"), ("receipt.png", b"receipt")]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &pending)).await, Outcome::Pending);
    assert!(w.replies_for(104).is_empty());
    assert_eq!(w.marks(104), vec!["⏳"]);

    w.cdn_down.store(false, Ordering::SeqCst);
    let hist = FakeHistory::new(&w, vec![pending.clone()]);
    catch_up(&allow(), &inbox, &hist).await;
    assert_eq!(w.replies_for(104).len(), 1);
}

/// With the model down the reply is the failure notice only (no OCR text, no verdict), and a failed send changes neither the note nor the tick.
#[tokio::test]
async fn reply_without_model_or_on_send_failure_changes_nothing() {
    let inbox_a = scratch("reply-model-down");
    let w_a = World::new(&inbox_a);
    let down = spec(MAIN, MAIN_CH, 106, &[("receipt.png", b"receipt down")]);
    assert_eq!(ingest(&allow(), &inbox_a, &msg(&w_a, &down)).await, Outcome::Saved);
    let text = &w_a.replies_for(106)[0];
    assert!(!text.contains("MODELDOWN"), "{text}");
    assert!(!text.contains("Receipt:") && !text.contains("Budget"), "{text}");

    let inbox_b = scratch("reply-clean");
    let w_b = World::new(&inbox_b);
    let plain = spec(MAIN, MAIN_CH, 105, &[("receipt.png", b"receipt")]);
    assert_eq!(ingest(&allow(), &inbox_b, &msg(&w_b, &plain)).await, Outcome::Saved);

    let inbox_c = scratch("reply-send-fails");
    let w_c = World::new(&inbox_c);
    w_c.reply_fails.store(true, Ordering::SeqCst);
    assert_eq!(ingest(&allow(), &inbox_c, &msg(&w_c, &plain)).await, Outcome::Saved);
    assert_eq!(w_c.reacts(105), 1);
    assert!(!in_retry(&inbox_c, 105));
    assert_eq!(
        fs::read(note(&inbox_c, 105)).unwrap(),
        fs::read(note(&inbox_b, 105)).unwrap()
    );
}

#[tokio::test]
async fn pending_gets_hourglass_then_tick_on_retry() {
    let inbox = scratch("hourglass-tick");
    let w = World::new(&inbox);
    let s = spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]);
    w.cdn_down.store(true, Ordering::SeqCst);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Pending);
    assert_eq!(w.marks(100), vec!["⏳"]);
    assert!(in_retry(&inbox, 100));
    w.cdn_down.store(false, Ordering::SeqCst);
    // The retry runs on a fetched message, which carries the bot's own reactions.
    let h = FakeHistory::new(&w, vec![s.clone()]);
    catch_up(&allow(), &inbox, &h).await;
    assert_eq!(w.marks(100), vec!["✅"]);
    assert!(retry_entries(&inbox).is_empty());
    assert_eq!(
        w.ops_for(100),
        vec![
            ("unreact", "✅".to_string()),
            ("unreact", "\u{26A0}\u{FE0F}".to_string()),
            ("react", "⏳".to_string()),
            ("unreact", "⏳".to_string()),
            ("unreact", "\u{26A0}\u{FE0F}".to_string()),
            ("react", "✅".to_string())
        ]
    );
}

#[tokio::test]
async fn failed_unreact_still_adds_tick_beside_hourglass() {
    // Pins the accepted trade-off: when Discord refuses the unreact of ⏳, the
    // ✅ is still added, so ✅ always means the note is saved (README).
    let inbox = scratch("unreact-fails-tick");
    let w = World::new(&inbox);
    let s = spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]);
    w.cdn_down.store(true, Ordering::SeqCst);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Pending);
    assert_eq!(w.marks(100), vec!["⏳"]);
    w.cdn_down.store(false, Ordering::SeqCst);
    w.unreact_fails.store(true, Ordering::SeqCst);
    let h = FakeHistory::new(&w, vec![s.clone()]);
    catch_up(&allow(), &inbox, &h).await;
    assert_eq!(w.marks(100), vec!["⏳", "✅"]);
    assert!(note(&inbox, 100).is_file());
    assert!(retry_entries(&inbox).is_empty());
    assert_eq!(
        w.ops_for(100),
        vec![
            ("unreact", "✅".to_string()),
            ("unreact", "\u{26A0}\u{FE0F}".to_string()),
            ("react", "⏳".to_string()),
            ("unreact", "⏳".to_string()),
            ("unreact", "\u{26A0}\u{FE0F}".to_string()),
            ("react", "✅".to_string())
        ]
    );
}

#[tokio::test]
async fn failed_write_gets_warning_then_tick_when_it_works() {
    let inbox = scratch("warn-tick");
    let w = World::new(&inbox);
    let s = spec(MAIN, MAIN_CH, 100, &[]);
    fs::create_dir_all(note(&inbox, 100)).unwrap();
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Failed);
    assert_eq!(w.marks(100), vec!["⚠\u{FE0F}"]);
    assert!(!note(&inbox, 100).is_file());
    assert!(!in_retry(&inbox, 100));
    fs::remove_dir(note(&inbox, 100)).unwrap();
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Saved);
    assert_eq!(w.marks(100), vec!["✅"]);
}

#[tokio::test]
async fn failed_write_with_failed_download_is_queued_with_hourglass() {
    let inbox = scratch("warn-pending");
    let w = World::new(&inbox);
    w.cdn_down.store(true, Ordering::SeqCst);
    let s = spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]);
    fs::create_dir_all(note(&inbox, 100)).unwrap();
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Pending);
    assert_eq!(w.marks(100), vec!["⏳"]);
    assert!(in_retry(&inbox, 100));
    assert!(!note(&inbox, 100).is_file());
}

/// One run of the three scenarios; returns what disk and outcomes looked like.
async fn run_scenarios(name: &str, fail: bool) -> (Vec<Outcome>, Vec<(u64, u64)>, Option<u64>, Vec<bool>, Vec<Vec<String>>) {
    let inbox = scratch(name);
    let w = World::new(&inbox);
    let h = FakeHistory::new(
        &w,
        vec![
            spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]),
            spec(MAIN, MAIN_CH, 200, &[]),
            spec(MAIN, MAIN_CH, 300, &[]),
        ],
    );
    w.react_fails.store(fail, Ordering::SeqCst);
    w.unreact_fails.store(fail, Ordering::SeqCst);
    let mut outs = Vec::new();
    w.cdn_down.store(true, Ordering::SeqCst);
    fs::create_dir_all(note(&inbox, 200)).unwrap();
    outs.push(ingest(&allow(), &inbox, &msg(&w, &spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]))).await);
    outs.push(ingest(&allow(), &inbox, &msg(&w, &spec(MAIN, MAIN_CH, 200, &[]))).await);
    catch_up(&allow(), &inbox, &h).await;
    w.cdn_down.store(false, Ordering::SeqCst);
    fs::remove_dir(note(&inbox, 200)).unwrap();
    catch_up(&allow(), &inbox, &h).await;
    outs.push(ingest(&allow(), &inbox, &msg(&w, &spec(MAIN, MAIN_CH, 300, &[]))).await);
    let notes = [100, 200, 300].iter().map(|i| note(&inbox, *i).is_file()).collect();
    let marks = [100, 200, 300].iter().map(|i| w.marks(*i)).collect();
    (outs, retry_entries(&inbox), read_cursor(&inbox, MAIN_CH), notes, marks)
}

#[tokio::test]
async fn reaction_failures_never_change_outcome_cursor_or_retry() {
    let good = run_scenarios("react-ok", false).await;
    let bad = run_scenarios("react-fail", true).await;
    assert_eq!(good.0, bad.0);
    assert_eq!(good.1, bad.1);
    assert_eq!(good.2, bad.2);
    assert_eq!(good.3, bad.3);
    assert!(good.4.iter().all(|m| m.len() == 1), "{:?}", good.4);
    assert!(bad.4.iter().all(|m| m.is_empty()), "{:?}", bad.4);
}

#[tokio::test]
async fn failed_retry_write_keeps_hourglass_and_retry_entry() {
    let inbox = scratch("retry-write-fails");
    let w = World::new(&inbox);
    let s = spec(MAIN, MAIN_CH, 100, &[("bad.jpg", b"b")]);
    w.cdn_down.store(true, Ordering::SeqCst);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &s)).await, Outcome::Pending);
    w.cdn_down.store(false, Ordering::SeqCst);
    fs::create_dir_all(note(&inbox, 100).with_extension("md.tmp")).unwrap();
    let h = FakeHistory::new(&w, vec![s.clone()]);
    catch_up(&allow(), &inbox, &h).await;
    assert!(in_retry(&inbox, 100));
    assert_eq!(w.marks(100), vec!["⏳"]);
}

// ---- Chat replies and vault filing (#31). Temp dirs and a made-up `Private` folder only. ----

/// Answers from a script, the way MiMo would, and records every user prompt it was sent.
struct FakeChat {
    answers: Mutex<VecDeque<Result<String, String>>>,
    prompts: Mutex<Vec<String>>,
}

impl FakeChat {
    fn new(answers: Vec<Result<String, String>>) -> Arc<FakeChat> {
        Arc::new(FakeChat {
            answers: Mutex::new(answers.into()),
            prompts: Mutex::new(Vec::new()),
        })
    }
    fn calls(&self) -> usize {
        self.prompts.lock().unwrap().len()
    }
}

impl Chat for FakeChat {
    fn ask(&self, _system: &str, user: &str) -> Result<String, String> {
        self.prompts.lock().unwrap().push(user.to_string());
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("script empty".to_string()))
    }
}

fn answer(folder: &str, title: &str, tags: &str, reply: &str) -> Result<String, String> {
    Ok(format!("FOLDER: {folder}\nTITLE: {title}\nTAGS: {tags}\nREPLY:\n{reply}\n"))
}

fn spec_text(author: u64, channel: u64, id: u64, text: &str, files: &[(&str, &[u8])]) -> Spec {
    let mut s = spec(author, channel, id, files);
    s.text = text.to_string();
    s
}

/// The vault root is the parent of the scratch Inbox. Every target folder exists, plus `Private` with one file.
fn vault_root(inbox: &Path) -> PathBuf {
    let root = inbox.parent().unwrap().to_path_buf();
    for t in TARGETS.iter() {
        fs::create_dir_all(root.join(t.folder)).unwrap();
    }
    fs::create_dir_all(root.join("Private")).unwrap();
    fs::write(root.join("Private/note.md"), "private\n").unwrap();
    root
}

fn filer(inbox: &Path, chat: &Arc<FakeChat>) -> Filer {
    Filer {
        chat: chat.clone(),
        vaults: Vaults::new(vault_root(inbox)),
        last_notice: Default::default(),
        window: Default::default(),
    }
}

fn root_of(inbox: &Path) -> PathBuf {
    inbox.parent().unwrap().to_path_buf()
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Every regular file under `dir`, recursively.
fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// A text DM is filed into its folder and answered in one message; no tick is added.
#[tokio::test]
async fn text_dm_filed_and_replied_no_tick() {
    let inbox = scratch("chat-filed");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer(
        "Citadel/Ideas",
        "Solar fan idea",
        "electronics",
        "That is a solar fan project. Filed under Citadel.",
    )]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 300, "sketch of a solar fan", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert!(!note(&inbox, 300).exists(), "note still in Inbox");
    let folder = root_of(&inbox).join("Citadel/Ideas");
    let filed = files_in(&folder);
    assert_eq!(filed, vec!["2026-10-13 Solar fan idea.md"], "{filed:?}");
    let text = fs::read_to_string(folder.join(&filed[0])).unwrap();
    assert!(text.contains("sketch of a solar fan") && text.contains("Hub: [[Ideas]]"), "{text}");
    let replies = w.replies_for(300);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0], "That is a solar fan project. Filed under Citadel.", "{}", replies[0]);
    assert!(w.ops_for(300).contains(&("react", discord_inbox::ingest::FILED_MARK.to_string())), "{:?}", w.ops_for(300));
    assert!(replies[0].contains("That is a solar fan project"), "{}", replies[0]);
    assert_eq!(w.marks(300), vec!["🐾"], "{:?}", w.marks(300));
    assert_eq!(w.reacts(300), 1);
    assert_eq!(chat.calls(), 1);
}

/// A filed note is never answered or filed again, by a second live pass or by catch-up, and catch-up moves past it.
#[tokio::test]
async fn filed_note_is_skipped_on_second_pass_and_catch_up() {
    let inbox = scratch("chat-skip");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Citadel/Ideas", "Solar fan idea", "electronics", "Filed.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 310, "sketch of a solar fan", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Skipped);
    let hist = FakeHistory::new(&w, vec![s.clone()]);
    catch_up_with(&allow(), &inbox, &hist, &f).await;
    assert_eq!(chat.calls(), 1, "a filed note was answered again");
    assert_eq!(w.replies_for(310).len(), 1);
    assert_eq!(files_in(&root_of(&inbox).join("Citadel/Ideas")).len(), 1, "filed again");
    assert_eq!(read_cursor(&inbox, MAIN_CH), Some(310), "catch-up stopped at a filed note");
}

/// A failed model call keeps the note in Inbox and says so in one reply (no silent tick, nothing filed).
#[tokio::test]
async fn chat_failure_falls_back_to_tick() {
    let inbox = scratch("chat-fail");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![Err("http 404".to_string())]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 320, "hello there", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert!(note(&inbox, 320).is_file(), "note left Inbox without a filing");
    assert_eq!(w.marks(320), vec!["✅"]);
    let replies = w.replies_for(320);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(replies[0].starts_with("Sorry, my brain's a little foggy right now 😿") && !replies[0].contains("http 404"), "{}", replies[0]);
    assert!(files_in(&root_of(&inbox).join("Citadel/Ideas")).is_empty());
    assert_eq!(chat.calls(), 1);
}

/// `Private`, or any path-like folder, is not a target: the note stays in Inbox and the reply says so. No tick.
#[tokio::test]
async fn off_list_folder_keeps_note_in_inbox_but_replies() {
    let inbox = scratch("chat-private");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Private", "Private thing", "", "Kept it here."),
        answer("../Private", "Private thing", "", "Kept it here too."),
    ]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    let before = files_in(&root.join("Private"));
    for id in [330u64, 331] {
        let s = spec_text(MAIN, MAIN_CH, id, "my knee hurts", &[]);
        assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
        assert!(note(&inbox, id).is_file(), "{id} left Inbox");
        assert!(w.marks(id).is_empty(), "{:?}", w.marks(id));
        let replies = w.replies_for(id);
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert!(replies[0].starts_with("Kept it here"), "{}", replies[0]);
    }
    assert_eq!(files_in(&root.join("Private")), before);
    assert_eq!(fs::read_to_string(root.join("Private/note.md")).unwrap(), "private\n");
}

/// A failed send falls back to the tick, and the note stays filed (the move already happened).
#[tokio::test]
async fn reply_send_failure_falls_back_to_tick_note_stays_filed() {
    let inbox = scratch("chat-send-fail");
    let w = World::new(&inbox);
    w.reply_fails.store(true, Ordering::SeqCst);
    let chat = FakeChat::new(vec![answer("Citadel/Ideas", "Solar fan idea", "electronics", "Filed.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 340, "sketch of a solar fan", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert!(!note(&inbox, 340).exists(), "note should be filed");
    assert_eq!(files_in(&root_of(&inbox).join("Citadel/Ideas")).len(), 1);
    assert!(dump::filed_to(&inbox, 340).is_some());
    assert_eq!(w.marks(340), vec!["✅"]);
    assert_eq!(w.replies_for(340).len(), 1);
    assert!(!in_retry(&inbox, 340));
}

/// Receipt: the verdict and the chat reply are one message, card digits masked, and the prompt never had them.
#[tokio::test]
async fn receipt_gets_verdict_and_chat_in_one_masked_reply() {
    let inbox = scratch("chat-receipt");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer(
        "State Street",
        "Trader Joes groceries",
        "receipts",
        "Logged your Trader Joe's run. Card 4111 1111 1111 1111",
    )]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 350, "receipt", &[("receipt.png", b"receipt card")]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let replies = w.replies_for(350);
    assert_eq!(replies.len(), 1, "{replies:?}");
    let text = &replies[0];
    assert!(text.starts_with("Logged your Trader Joe's run. Card **** **** **** 1111\n\nReceipt: Trader Joe's"), "{text}");
    assert!(text.contains("Logged your Trader Joe's run."), "{text}");
    assert!(text.contains("Receipt: Trader Joe's · $12.34 · groceries"), "{text}");
    assert!(text.contains("**** **** **** 1111") && !text.contains("4111"), "{text}");
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts.iter().all(|p| !p.contains("4111")), "{prompts:?}");
    assert_eq!(w.marks(350), vec!["🐾"], "{:?}", w.marks(350));
}

/// A pending DM gets the hourglass first and is not answered early; when the retry completes, the hourglass goes and the DM is filed and answered.
#[tokio::test]
async fn retry_completion_clears_hourglass_when_replied() {
    let inbox = scratch("chat-retry");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Berkshire", "Photo of a bill", "business", "Saved the bill.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 360, "photo of a bill", &[("bad.jpg", b"b")]);
    w.cdn_down.store(true, Ordering::SeqCst);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Pending);
    assert_eq!(chat.calls(), 0, "a pending message was answered early");
    assert_eq!(w.marks(360), vec!["⏳"]);
    w.cdn_down.store(false, Ordering::SeqCst);
    let hist = FakeHistory::new(&w, vec![s.clone()]);
    catch_up_with(&allow(), &inbox, &hist, &f).await;
    assert_eq!(w.marks(360), vec!["🐾"], "{:?}", w.marks(360));
    assert!(
        w.ops_for(360).iter().any(|(k, e)| *k == "unreact" && e == "⏳"),
        "{:?}",
        w.ops_for(360)
    );
    assert_eq!(w.replies_for(360).len(), 1);
    assert!(!in_retry(&inbox, 360));
    assert_eq!(files_in(&root_of(&inbox).join("Berkshire")).len(), 2, "note and attachment");
}

/// Hostile or random model output never moves anything outside Inbox into a non-target folder, never touches
/// Private, and every message ends either filed (ledger) or in Inbox, never both and never neither.
#[tokio::test]
async fn filing_never_leaves_inbox_except_into_targets() {
    let inbox = scratch("chat-invariant");
    let w = World::new(&inbox);
    let root = root_of(&inbox);

    let mut answers: Vec<Result<String, String>> = [
        "Private",
        "../Private",
        "/tmp",
        "Citadel/Ideas/../../Private",
        "Private/note.md",
        "`Private`",
        "citadel",
        "",
        "Fidelity/Want to Make/../../Private",
        "Inbox",
    ]
    .iter()
    .map(|f| answer(f, "../../x", "../x, #y", "ok"))
    .collect();
    answers.push(answer("State Street", &"A".repeat(10_000), "finance", "ok"));
    answers.push(Ok(format!("TITLE: {}", "A".repeat(10_000))));
    answers.push(Ok("FOLDER: ! Todo\nTITLE: /etc/passwd\nREPLY:\nok".to_string()));
    answers.push(Ok("{\"folder\":\"Private\"}".to_string()));
    answers.push(Err("timeout".to_string()));

    // Seeded LCG, so the run is the same every time.
    let mut seed: u64 = 31;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    let alphabet: &[u8] = b"/.abcPrivate ` :\n-_";
    for _ in 0..200 {
        let len = next() % 40;
        let s: String = (0..len).map(|_| alphabet[next() % alphabet.len()] as char).collect();
        answers.push(answer(&s, &s, &s, &s));
    }

    let total = answers.len();
    let chat = FakeChat::new(answers);
    // Baseline after `filer`, which creates the `Private/note.md` stand-in. A baseline taken before it
    // reads an empty folder and reports the stand-in as a leak at the first call. Verified in a throwaway
    // copy of the PR head: with this order the whole loop passes, so no production path writes there.
    let f = filer(&inbox, &chat);
    let private_before = files_in(&root.join("Private"));
    for i in 0..total {
        let id = 400_000 + i as u64;
        let s = spec_text(MAIN, MAIN_CH, id, "random text", &[]);
        let out = ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await;
        assert_eq!(out, Outcome::Saved, "id {id}");
        assert_eq!(files_in(&root.join("Private")), private_before, "Private changed at id {id}");
        let filed = dump::filed_to(&inbox, id).is_some();
        let in_inbox = note(&inbox, id).is_file();
        assert!(filed ^ in_inbox, "id {id}: filed={filed} in_inbox={in_inbox}");
        // Answers 0..=5 name the private folder (plain, `../`, a file inside it, backticked, traversal) or
        // an absolute path. None may file the note: each must stay in Inbox.
        if i <= 5 {
            assert!(in_inbox && !filed, "id {id}: model named the private folder, note must stay in Inbox");
        }
        for p in all_files(&root) {
            let rel = p.strip_prefix(&root).unwrap();
            if rel.starts_with("Inbox") || rel.starts_with("Private") {
                continue;
            }
            let parent = rel.parent().unwrap().to_string_lossy().into_owned();
            assert!(
                TARGETS.iter().any(|t| t.folder == parent),
                "file outside the targets after id {id}: {}",
                rel.display()
            );
        }
    }
    assert_eq!(chat.calls(), total);
}

/// Card digits typed in the DM text (not only in OCR) are masked before the model sees them.
#[tokio::test]
async fn text_dm_card_digits_are_masked_before_the_model() {
    let inbox = scratch("chat-text-mask");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Berkshire", "Card note", "business", "Noted.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 380, "card 4111 1111 1111 1111 thanks", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert!(!prompts[0].contains("4111"), "card prefix reached the model: {prompts:?}");
    assert!(prompts[0].contains("1111"), "masked tail missing: {prompts:?}");
}

/// The live event and the catch-up both ingest one DM at once: exactly one reply, one model call, never "Kept in Inbox" for a filed note.
#[tokio::test]
async fn concurrent_duplicate_ingest_replies_once() {
    let inbox = scratch("chat-race");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Goldman", "Race test", "career", "Filed the race test.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 400, "a file", &[("notes.txt", b"hello")]);
    let (live, catch) = (msg(&w, &s), msg(&w, &s));
    let al = allow();
    let (a, b) = tokio::join!(ingest_with(&al, &inbox, &live, &f), ingest_with(&al, &inbox, &catch, &f));
    let mut outcomes = vec![a, b];
    outcomes.sort_by_key(|o| format!("{o:?}"));
    assert_eq!(outcomes, vec![Outcome::Saved, Outcome::Skipped], "{outcomes:?}");
    let replies = w.replies_for(400);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(!replies[0].starts_with("Kept in Inbox"), "{replies:?}");
    assert_eq!(chat.calls(), 1, "the model ran {} times", chat.calls());
}

// ---- Answer, Shopping List and the failure notice (#40). Temp dirs and a made-up `Private` folder only. ----

/// A failed model call with a receipt still sends exactly one reply (P4): the notice, not a second message.
#[tokio::test]
async fn chat_failure_with_receipt_sends_one_reply() {
    let inbox = scratch("chat-fail-receipt");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![Err("http 500".to_string())]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 325, "receipt for groceries", &[("receipt.jpg", b"receipt bytes")]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let replies = w.replies_for(325);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(replies[0].starts_with("Sorry, my brain's a little foggy right now 😿") && !replies[0].contains("http 500"), "{}", replies[0]);
    assert_eq!(w.marks(325), vec!["✅"]);
}

/// A question is answered from the bot's own notes and files nothing. The lookup prompt carries the
/// Shopping List and the Inbox notes, and never the Private folder.
#[tokio::test]
async fn question_is_answered_not_filed() {
    let inbox = scratch("chat-answer");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Answer", "Costco list lookup", "shopping", "I don't have it"),
        Ok("Eggs, masking tape".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    let shop = root.join("State Street/Cash Flow/Spending");
    fs::create_dir_all(&shop).unwrap();
    fs::create_dir_all(&inbox).unwrap();
    fs::write(shop.join("Shopping List.md"), "- [ ] Eggs\n").unwrap();
    fs::write(inbox.join("2026-10-12 tape.md"), "masking tape\n").unwrap();
    fs::write(inbox.join(".filed"), format!("999 {}\n", root.join("Private/note.md").display())).unwrap();

    let s = spec_text(MAIN, MAIN_CH, 330, "what is on my Costco list?", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert!(note(&inbox, 330).is_file(), "question note left Inbox");
    for t in TARGETS.iter() {
        assert!(!files_in(&root.join(t.folder)).iter().any(|n| n.ends_with(".md")), "filed into {}", t.folder);
    }

    let replies = w.replies_for(330);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0], "Eggs, masking tape", "the reply is the model's answer only (#45)");
    assert!(w.marks(330).is_empty(), "{:?}", w.marks(330));
    assert_eq!(chat.calls(), 2);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts[1].contains("what is on my Costco list?"), "{}", prompts[1]);
    assert!(!prompts[1].contains("- [ ] Eggs"), "the notes are read by the tools, not bundled (#49): {}", prompts[1]);
    assert!(!prompts[1].to_lowercase().contains("private"), "{}", prompts[1]);
}

/// If the lookup pass fails, the answer from the first pass is still sent, labelled as unchecked.
#[tokio::test]
async fn lookup_failure_still_answers() {
    let inbox = scratch("chat-answer-fail");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Answer", "Costco list lookup", "shopping", "model guess"),
        Err("timeout".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 331, "what is on my Costco list?", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let replies = w.replies_for(331);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(!replies[0].starts_with("Answered"), "{}", replies[0]);
    assert!(replies[0].starts_with("model guess"), "{}", replies[0]);
    assert!(replies[0].ends_with("(I couldn't peek at your notes just now, sorry!)"), "{}", replies[0]);
    assert!(!replies[0].contains("lookup failed"), "{}", replies[0]);
    assert!(note(&inbox, 331).is_file(), "question note left Inbox");
    assert_eq!(chat.calls(), 2);
}

/// A list item is appended to the running note, not filed as a new note. The Inbox copy is dropped.
#[tokio::test]
async fn list_item_appends_to_running_note() {
    let inbox = scratch("chat-running");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Shopping List", "Costco list", "shopping", "Added.")]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    let shop = root.join("State Street/Cash Flow/Spending");
    fs::create_dir_all(&shop).unwrap();
    let list = shop.join("Shopping List.md");
    fs::write(&list, "## Costco\n- [ ] Eggs\n\n**Hub:** [[Home]]\n").unwrap();

    let s = spec_text(MAIN, MAIN_CH, 340, "eggs milk from costco", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let text = fs::read_to_string(&list).unwrap();
    assert!(text.contains("- [ ] eggs milk from costco *(Discord 2026-10-13)*"), "{text}");
    assert!(text.find("- [ ] eggs").unwrap() < text.find("**Hub:**").unwrap(), "{text}");
    assert!(!note(&inbox, 340).exists(), "Inbox copy left behind");
    assert!(!files_in(&root.join("State Street")).iter().any(|n| n.ends_with(".md")), "filed a new note");
    assert!(fs::read_to_string(inbox.join(".filed")).unwrap().contains("Shopping List.md"));
    let replies = w.replies_for(340);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0], "Added.", "{}", replies[0]);
    assert!(w.ops_for(340).contains(&("react", discord_inbox::ingest::FILED_MARK.to_string())), "{:?}", w.ops_for(340));
    assert_eq!(chat.calls(), 1);
}

/// A list message with an attachment is filed as a note, so the file is kept; the running note is untouched.
#[tokio::test]
async fn running_with_attachment_files_a_note() {
    let inbox = scratch("chat-running-attach");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Shopping List", "Costco list", "shopping", "Filed.")]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    let shop = root.join("State Street/Cash Flow/Spending");
    fs::create_dir_all(&shop).unwrap();
    let list = shop.join("Shopping List.md");
    let before = "## Costco\n- [ ] Eggs\n\n**Hub:** [[Home]]\n";
    fs::write(&list, before).unwrap();

    let s = spec_text(MAIN, MAIN_CH, 341, "groceries", &[("receipt.txt", b"receipt")]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(fs::read_to_string(&list).unwrap(), before);
    assert!(files_in(&root.join("State Street")).contains(&"2026-10-13 Costco list.md".to_string()));
    let replies = w.replies_for(341);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0], "Filed.", "{}", replies[0]);
    assert!(w.ops_for(341).contains(&("react", discord_inbox::ingest::FILED_MARK.to_string())), "{:?}", w.ops_for(341));
}

/// With no running note on disk, a list message is filed as an ordinary note in State Street.
#[tokio::test]
async fn running_note_missing_files_a_note() {
    let inbox = scratch("chat-running-missing");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Shopping List", "Costco list", "shopping", "Filed.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 342, "eggs from costco", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert!(files_in(&root_of(&inbox).join("State Street")).contains(&"2026-10-13 Costco list.md".to_string()));
    let replies = w.replies_for(342);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0], "Filed.", "{}", replies[0]);
    assert!(w.ops_for(342).contains(&("react", discord_inbox::ingest::FILED_MARK.to_string())), "{:?}", w.ops_for(342));
}

/// One notice per failure episode: a repeat of the same chat error gets ✅ only, and a success ends the episode.
#[tokio::test]
async fn same_chat_error_notices_once_until_chat_works() {
    let inbox = scratch("chat-once");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        Err("http 401".to_string()),
        Err("http 401".to_string()),
        answer("Private", "Note", "misc", "ok"),
        Err("http 401".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    for id in 340..344 {
        let s = spec_text(MAIN, MAIN_CH, id, "plain message", &[]);
        ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await;
    }
    let first = w.replies_for(340);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(first[0].starts_with("Sorry, my brain's a little foggy right now 😿") && !first[0].contains("http 401"), "{}", first[0]);
    assert!(w.replies_for(341).is_empty(), "{:?}", w.replies_for(341));
    assert_eq!(w.marks(341), vec!["✅"]);
    let third = w.replies_for(342);
    assert_eq!(third.len(), 1, "{third:?}");
    assert!(third[0].contains("ok"), "{}", third[0]);
    let fourth = w.replies_for(343);
    assert_eq!(fourth.len(), 1, "{fourth:?}");
    assert!(fourth[0].starts_with("Sorry, my brain's a little foggy right now 😿") && !fourth[0].contains("http 401"), "{}", fourth[0]);
}

/// A question the model routes to a running list is answered, and the list is left alone.
#[tokio::test]
async fn question_routed_to_running_list_is_answered() {
    let inbox = scratch("chat-running-q");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Shopping List", "Costco run", "shopping", "Added."),
        Ok("Eggs, masking tape".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    let shop = root.join("State Street/Cash Flow/Spending");
    fs::create_dir_all(&shop).unwrap();
    fs::write(shop.join("Shopping List.md"), "- [ ] Eggs\n").unwrap();

    let s = spec_text(MAIN, MAIN_CH, 350, "whats on my costco like list?", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let text = fs::read_to_string(shop.join("Shopping List.md")).unwrap();
    assert!(!text.contains("whats on my costco"), "{text}");
    let replies = w.replies_for(350);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0], "Eggs, masking tape", "{}", replies[0]);
}

/// Different errors inside the notice gap still give one notice in total.
#[tokio::test]
async fn different_errors_inside_gap_notice_once() {
    let inbox = scratch("chat-gap");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![Err("http 401".to_string()), Err("http 500".to_string())]);
    let f = filer(&inbox, &chat);
    for id in 360..362 {
        let s = spec_text(MAIN, MAIN_CH, id, "plain message", &[]);
        ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await;
    }
    let total = w.replies_for(360).len() + w.replies_for(361).len();
    assert_eq!(total, 1, "{:?} {:?}", w.replies_for(360), w.replies_for(361));
}

/// A live DM shows "typing" while the model works (#45).
#[tokio::test]
async fn live_dm_shows_typing_while_the_model_works() {
    let inbox = scratch("typing-live");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Answer", "Costco list", "shopping", "Eggs."), Ok("Eggs, masking tape".into())]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 370, "what is on my costco list?", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert!(w.typing_count(370) >= 1, "no typing shown for a live DM");
    assert_eq!(w.replies_for(370).len(), 1);
}

/// Catch-up never shows typing: old DMs are replayed, not waited on (#45).
#[tokio::test]
async fn catch_up_never_shows_typing() {
    let inbox = scratch("typing-catch-up");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Answer", "Costco list", "shopping", "Eggs."), Ok("Eggs, masking tape".into())]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 371, "what is on my costco list?", &[]);
    let h = FakeHistory::new(&w, vec![s]);
    catch_up_with(&allow(), &inbox, &h, &f).await;
    assert_eq!(w.typing_count(371), 0, "typing sent during catch-up");
    assert_eq!(w.replies_for(371).len(), 1, "{:?}", w.replies_for(371));
}

/// The lookup reads the IOU ledger alongside the running notes (#45).
#[tokio::test]
async fn lookup_reads_the_iou_ledger() {
    let inbox = scratch("chat-ledger");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Answer", "William debt", "money", "Checking."),
        Ok("William owes you $12.00.".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    let rec = root.join("State Street/Assets/Receivables");
    fs::create_dir_all(&rec).unwrap();
    fs::write(rec.join("Who Owes Me.md"), "| Who | Owes |\n| William | $12.00 |\n").unwrap();
    let mut before = Vec::new();
    tree_of(&root, &inbox, &mut before);
    before.sort();

    let s = spec_text(MAIN, MAIN_CH, 380, "how much does willam owe me", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert_eq!(chat.calls(), 2);
    assert!(prompts[1].contains("how much does willam owe me"), "{}", prompts[1]);
    assert_eq!(w.replies_for(380), vec!["William owes you $12.00.".to_string()]);
    let mut after = Vec::new();
    tree_of(&root, &inbox, &mut after);
    after.sort();
    assert_eq!(after, before, "the lookup filed or wrote something under the vault (#49)");
}

/// No ledger file: the lookup still runs over the running notes (#45).
#[tokio::test]
async fn lookup_works_without_the_iou_ledger() {
    let inbox = scratch("chat-no-ledger");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Answer", "William debt", "money", "Checking."),
        Ok("No record of that.".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 381, "how much does willam owe me", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(w.replies_for(381), vec!["No record of that.".to_string()]);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts[1].contains("how much does willam owe me"), "{}", prompts[1]);
    assert!(!prompts[1].contains("Notes:"), "the tools read the notes now, not a bundle (#49): {}", prompts[1]);
}

/// A long DM sent as `message.txt` with an empty body is routed on the file's text, never answered as a lookup (#46 scope add).
#[tokio::test]
async fn text_attachment_routes_on_its_text_not_a_lookup() {
    let inbox = scratch("attach-text");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Citadel/Ideas", "Demo announcement", "announce", "Filed it.")]);
    let f = filer(&inbox, &chat);
    let body = "Stormy, the Discord bot is ready for the demo on Friday.";
    let s = spec_text(MAIN, MAIN_CH, 390, "", &[("message.txt", body.as_bytes())]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(chat.calls(), 1, "a lookup ran for an attachment");
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts[0].contains(body), "{}", prompts[0]);
    let replies = w.replies_for(390);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(replies[0].starts_with("Filed it."), "{}", replies[0]);
    assert!(w.ops_for(390).contains(&("react", discord_inbox::ingest::FILED_MARK.to_string())), "{:?}", w.ops_for(390));
    let filed = all_files(&root_of(&inbox).join("Citadel/Ideas"));
    assert!(
        filed.iter().filter(|p| p.extension().is_some_and(|x| x == "md")).any(|p| fs::read_to_string(p).unwrap_or_default().contains(body)),
        "filed note body misses the attachment text"
    );
}

/// A DM with no text and no readable text is saved and acknowledged; the answer model is never called (#46 scope add).
#[tokio::test]
async fn blank_dm_never_calls_the_model() {
    let inbox = scratch("blank-dm");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Answer", "Lookup", "", "Lookup answer about unrelated notes.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 391, "", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(chat.calls(), 0, "the model was called for a blank DM");
    assert_eq!(w.replies_for(391), vec![discord_inbox::ingest::BLANK_REPLY.to_string()]);
}

/// The attachment text is read up to 20 000 chars, cut on a char boundary (#46 scope add).
#[tokio::test]
async fn text_attachment_is_capped_at_20000_chars() {
    let inbox = scratch("attach-cap");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Citadel/Ideas", "Long", "", "Filed.")]);
    let f = filer(&inbox, &chat);
    let long = "é".repeat(30_000);
    let s = spec_text(MAIN, MAIN_CH, 392, "", &[("message.txt", long.as_bytes())]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let filed = all_files(&root_of(&inbox).join("Citadel/Ideas"));
    let text: String = filed
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .map(|p| fs::read_to_string(p).unwrap_or_default())
        .collect();
    assert!(text.contains(&"é".repeat(20_000)), "cap is below 20 000 chars");
    assert!(!text.contains(&"é".repeat(20_001)), "cap is above 20 000 chars");
}

/// The model picked a folder but the note could not be filed: the reply says so in one line (#46 fix round).
#[tokio::test]
async fn couldnt_file_line_when_filing_fails() {
    let inbox = scratch("cant-file");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Citadel/Ideas", "Solar fan idea", "electronics", "Filed.")]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    fs::remove_dir_all(root.join("Citadel/Ideas")).unwrap();
    fs::write(root.join("Citadel/Ideas"), "not a folder\n").unwrap();
    let s = spec_text(MAIN, MAIN_CH, 393, "solar fan idea", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(
        w.replies_for(393),
        vec!["Filed.\n\nHmm, I couldn't file that one, so it's safe in your Inbox for now.".to_string()]
    );
}

fn tree_of(dir: &Path, skip: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.starts_with(skip) {
            continue;
        }
        out.push(p.clone());
        if p.is_dir() && !p.is_symlink() {
            tree_of(&p, skip, out);
        }
    }
}

/// The Answer path is read-only (#49): the tool loop files nothing under the vault root. The intake record in `Inbox/` is written for every message, so it is skipped.
#[tokio::test]
async fn answer_path_writes_nothing_anywhere_under_root() {
    let inbox = scratch("answer-writes");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Answer", "pantry question", "food", "Checking."),
        Ok("Cod with soy sauce.".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    fs::create_dir_all(root.join("Fidelity/Pantry")).unwrap();
    fs::write(root.join("Fidelity/Pantry/Pantry.md"), "cod fillets, soy sauce\n").unwrap();
    let mut before = Vec::new();
    tree_of(&root, &inbox, &mut before);
    before.sort();
    let s = spec_text(MAIN, MAIN_CH, 382, "with the ingredients in my pantry what can i do with my cod?", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let mut after = Vec::new();
    tree_of(&root, &inbox, &mut after);
    after.sort();
    assert_eq!(after, before);
    assert_eq!(w.replies_for(382), vec!["Cod with soy sauce.".to_string()]);
}

// ---- Warm replies (#67). Temp dirs and a made-up `Private` folder only. ----

/// A filed text DM gets the model's reply and nothing else: no footer, and a 🐾 reaction on his message.
#[tokio::test]
async fn filed_reply_is_model_text_plus_paw() {
    let inbox = scratch("warm-filed");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Citadel/Ideas", "Solar fan idea", "electronics", "That is a solar fan project.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 400, "sketch of a solar fan", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let replies = w.replies_for(400);
    assert_eq!(replies, vec!["That is a solar fan project.".to_string()], "{replies:?}");
    let ops = w.ops_for(400);
    assert!(ops.contains(&("react", discord_inbox::ingest::FILED_MARK.to_string())), "{ops:?}");
    assert!(files_in(&root_of(&inbox).join("Citadel/Ideas")).contains(&"2026-10-13 Solar fan idea.md".to_string()));
}

/// A follow-up the model files under `Chat` goes nowhere: no note in any target folder, and the reply is the model text.
#[tokio::test]
async fn chat_decision_files_nothing() {
    let inbox = scratch("warm-chat");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Chat", "Bottle", "", "Oh nice, that's the one.")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 401, "it comes with the bottle tho", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(w.replies_for(401), vec!["Oh nice, that's the one.".to_string()]);
    assert!(!w.ops_for(401).iter().any(|(_, e)| e == discord_inbox::ingest::FILED_MARK), "{:?}", w.ops_for(401));
    let root = root_of(&inbox);
    for t in TARGETS.iter() {
        assert!(files_in(&root.join(t.folder)).is_empty(), "{} got a note", t.folder);
    }
}

/// The turn before reaches the next filing prompt, in the same channel only.
#[tokio::test]
async fn window_reaches_the_next_prompt() {
    let inbox = scratch("warm-window");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Citadel/Ideas", "Sodastream", "electronics", "Nice, filed under Citadel."),
        answer("Chat", "Bottle", "", "Lol right."),
        answer("Chat", "Other", "", "Hi there."),
    ]);
    let f = filer(&inbox, &chat);
    let s1 = spec_text(MAIN, MAIN_CH, 402, "sodastream terra 89.99", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s1), &f).await, Outcome::Saved);
    let s2 = spec_text(MAIN, MAIN_CH, 403, "it comes with the bottle tho", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s2), &f).await, Outcome::Saved);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    let p = &prompts[1];
    assert!(p.contains("Earlier in this chat"), "{p}");
    assert!(p.contains("Lionel: sodastream terra 89.99"), "{p}");
    assert!(p.contains("Stormy: Nice, filed under Citadel."), "{p}");
    assert!(p.contains("Now he says:\nit comes with the bottle tho"), "{p}");

    let other = spec_text(MAIN, ALT_CH, 404, "hello from another chat", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &other), &f).await, Outcome::Saved);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(!prompts[2].contains("Earlier in this chat"), "{}", prompts[2]);
}

/// The window also reaches the question path: every prompt after the first carries the earlier turn.
#[tokio::test]
async fn window_reaches_the_question_path() {
    let inbox = scratch("warm-question");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Citadel/Ideas", "Sodastream", "electronics", "Nice, filed under Citadel."),
        answer("Answer", "", "", ""),
        Ok("Get the Terra one.".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let s1 = spec_text(MAIN, MAIN_CH, 405, "sodastream terra 89.99", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s1), &f).await, Outcome::Saved);
    let s2 = spec_text(MAIN, MAIN_CH, 406, "which one should i get?", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s2), &f).await, Outcome::Saved);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts.len() >= 2, "{prompts:?}");
    for p in &prompts[1..] {
        assert!(p.contains("Earlier in this chat") && p.contains("Lionel: sodastream terra 89.99"), "{p}");
    }
}

/// A retried turn that arrives late sorts by its own stamp, and a turn after `now` is not shown.
#[test]
fn window_reads_oldest_first_when_a_catch_up_turn_arrives_late() {
    use discord_inbox::ingest::Window;
    let at = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc);
    let win = Window::default();
    win.push(5, "2026-10-14T12:02:00-04:00", "B later", "b reply");
    win.push(5, "2026-10-14T12:01:00-04:00", "A earlier", "a reply");
    let got: Vec<String> = win.recent_at(5, at("2026-10-14T12:05:00-04:00")).into_iter().map(|(him, _)| him).collect();
    assert_eq!(got, vec!["A earlier", "B later"], "window must read oldest first");
    let got = win.recent_at(5, at("2026-10-14T12:01:00-04:00"));
    assert_eq!(got.len(), 1, "a turn after the message leaked into its prompt: {got:?}");
    assert_eq!(got[0].0, "A earlier");
}

/// The window masks at its own boundary, keeps the last six turns oldest first, drops turns older than 30 minutes, and cuts long text.
#[test]
fn window_is_masked_and_bounded() {
    use discord_inbox::ingest::Window;
    let now = chrono::Utc::now().to_rfc3339();

    let win = Window::default();
    win.push(1, &now, "card 4111 1111 1111 1111 ssn 123-45-6789", "ok");
    let got = win.recent(1);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(!got[0].0.contains("4111") && !got[0].0.contains("123-45-6789"), "{got:?}");
    assert!(got[0].0.contains("**** **** **** 1111"), "{got:?}");

    let win = Window::default();
    for i in 0..8 {
        win.push(2, &now, &format!("t{i}"), "r");
    }
    let got = win.recent(2);
    assert_eq!(got.len(), 6, "{got:?}");
    assert_eq!(got.first().unwrap().0, "t2");
    assert_eq!(got.last().unwrap().0, "t7");

    let win = Window::default();
    let two_hours = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    win.push(3, &two_hours, "old", "x");
    assert!(win.recent(3).is_empty());

    let win = Window::default();
    win.push(4, &now, &"a".repeat(1000), &"b".repeat(1000));
    let got = win.recent(4);
    assert_eq!(got[0].0.chars().count(), 400);
    assert_eq!(got[0].1.chars().count(), 400);
}

/// A forward has no text of its own: its text comes from the snapshot, and typed text always wins.
#[test]
fn blank_forward_text_comes_from_snapshot() {
    use discord_inbox::ingest::forwarded_text;
    assert_eq!(forwarded_text("", &["IPI: the lab is closed Friday".into()]), "IPI: the lab is closed Friday");
    assert_eq!(forwarded_text("   ", &["one".into(), "two".into()]), "one\n\ntwo");
    assert_eq!(forwarded_text("hi", &["x".into()]), "hi");
    assert_eq!(forwarded_text("", &[]), "");
}

/// A question about a screenshot sees the screenshot's text (redacted), as the filing prompt does (#73).
#[tokio::test]
async fn question_about_a_screenshot_sends_the_redacted_ocr() {
    let inbox = scratch("question-ocr");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Answer", "Which one", "shopping", "Checking."),
        Ok("Go with the second one.".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 390, "which one should i get", &[("card.png", b"card bytes")]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(chat.calls(), 2);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts[1].contains("which one should i get"), "{}", prompts[1]);
    assert!(prompts[1].contains("Attached text:"), "{}", prompts[1]);
    assert!(prompts[1].contains("TRADER JOES"), "{}", prompts[1]);
    assert!(!prompts[1].contains("4111"), "card digits reached the model: {}", prompts[1]);
    let replies = w.replies_for(390);
    assert!(replies[0].starts_with("Go with the second one."), "{replies:?}");
    assert!(note(&inbox, 390).is_file(), "question note left Inbox");
}

/// A typed question longer than the filing cap keeps its tail on the question path: the lookup sees the whole text (#75).
#[tokio::test]
async fn long_question_keeps_its_tail_for_the_lookup() {
    let inbox = scratch("long-question");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Answer", "Long question", "misc", "Checking."),
        Ok("Here is the answer.".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let text = format!("{} TAILMARK?", "word ".repeat(1000));
    assert!(text.chars().count() > discord_inbox::vaults::PROMPT_CHARS);
    let s = spec_text(MAIN, MAIN_CH, 392, &text, &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(chat.calls(), 2);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts[1].contains("TAILMARK?"), "question tail cut before the lookup: {} chars", prompts[1].chars().count());
}

/// A question with no "?" that the model answers with a look-it-up promise is answered, not left hanging (#73).
#[tokio::test]
async fn question_without_mark_that_got_a_promise_is_answered() {
    let inbox = scratch("promise-answered");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Inbox", "William debt", "money", "Let me dig through your notes for that, give me a sec 🐾"),
        Ok("William owes you $12.00.".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let root = root_of(&inbox);
    let rec = root.join("State Street/Assets/Receivables");
    fs::create_dir_all(&rec).unwrap();
    fs::write(rec.join("Who Owes Me.md"), "| Who | Owes |\n| William | $12.00 |\n").unwrap();
    let mut before = Vec::new();
    tree_of(&root, &inbox, &mut before);
    before.sort();

    let s = spec_text(MAIN, MAIN_CH, 391, "how much does willam owe me", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(chat.calls(), 2);
    let prompts = chat.prompts.lock().unwrap().clone();
    assert!(prompts[1].contains("how much does willam owe me"), "{}", prompts[1]);
    assert_eq!(w.replies_for(391), vec!["William owes you $12.00.".to_string()]);
    assert!(w.marks(391).is_empty(), "{:?}", w.marks(391));
    assert!(note(&inbox, 391).is_file(), "question note left Inbox");
    let mut after = Vec::new();
    tree_of(&root, &inbox, &mut after);
    after.sort();
    assert_eq!(after, before, "the answer path filed or wrote something under the vault");
}

/// When the promised lookup fails, the bot does not repeat the promise back to him (#73).
#[tokio::test]
async fn promise_then_lookup_failure_does_not_repeat_the_promise() {
    let inbox = scratch("promise-fail");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![
        answer("Inbox", "William debt", "money", "Let me dig through your notes for that, give me a sec 🐾"),
        Err("timeout".to_string()),
    ]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 392, "how much does willam owe me", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    let replies = w.replies_for(392);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(!replies[0].contains("give me a sec"), "{}", replies[0]);
    assert!(replies[0].ends_with("(I couldn't peek at your notes just now, sorry!)"), "{}", replies[0]);
}

/// The promise net only fires when nothing was filed: a filed note with a promise-y reply still files (#73).
#[tokio::test]
async fn filed_note_with_a_promise_still_files() {
    let inbox = scratch("promise-filed");
    let w = World::new(&inbox);
    let chat = FakeChat::new(vec![answer("Citadel/Ideas", "Solar fan idea", "electronics", "I'll check what parts it needs later")]);
    let f = filer(&inbox, &chat);
    let s = spec_text(MAIN, MAIN_CH, 393, "solar fan idea for the balcony", &[]);
    assert_eq!(ingest_with(&allow(), &inbox, &msg(&w, &s), &f).await, Outcome::Saved);
    assert_eq!(chat.calls(), 1);
    assert!(w.ops_for(393).contains(&("react", discord_inbox::ingest::FILED_MARK.to_string())), "{:?}", w.ops_for(393));
}
