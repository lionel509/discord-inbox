//! Behaviour of `ingest` and `catch_up` against fakes. Fake IDs only.
use discord_inbox::allowlist;
use discord_inbox::allowlist::Allowlist;
use discord_inbox::dump::{
    self, add_retry, in_retry, note_is_complete, read_cursor, retry_entries, sha256_hex,
    write_cursor,
};
use discord_inbox::ingest::{catch_up, ingest, History, Inbound, Outcome, PAGE};
use std::collections::HashMap;
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
    files: Vec<(String, Vec<u8>)>,
}

fn spec(author: u64, channel: u64, id: u64, files: &[(&str, &[u8])]) -> Spec {
    Spec {
        author,
        channel,
        id,
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
            ops: Mutex::new(Vec::new()),
        })
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
        "hi".to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.spec.files.iter().map(|f| f.0.clone()).collect()
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.w.add_download(self.spec.id);
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
        Ok(picked.into_iter().map(|spec| msg(&self.w, &spec)).collect())
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
            Some(spec) => Ok(msg(&self.w, &spec)),
            None => Err("unknown message".to_string()),
        }
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
        Outcome::Failed
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
    assert!(text.contains("TRADER JOES"), "{text}");
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
    assert!(replies[0].contains("**** **** **** 1111"), "{}", replies[0]);
    assert!(!replies[0].contains("4111"), "{}", replies[0]);
}

/// Nothing to say means no reply: plain text, strangers, a pending receipt.
#[tokio::test]
async fn no_reply_when_nothing_to_say() {
    let inbox = scratch("reply-none");
    let w = World::new(&inbox);
    let good = spec(MAIN, MAIN_CH, 102, &[("good.txt", b"hello")]);
    assert_eq!(ingest(&allow(), &inbox, &msg(&w, &good)).await, Outcome::Saved);
    assert!(w.replies_for(102).is_empty());

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

/// With the model down the reply is OCR only, and a failed send changes neither the note nor the tick.
#[tokio::test]
async fn reply_without_model_or_on_send_failure_changes_nothing() {
    let inbox_a = scratch("reply-model-down");
    let w_a = World::new(&inbox_a);
    let down = spec(MAIN, MAIN_CH, 106, &[("receipt.png", b"receipt down")]);
    assert_eq!(ingest(&allow(), &inbox_a, &msg(&w_a, &down)).await, Outcome::Saved);
    let text = &w_a.replies_for(106)[0];
    assert!(text.contains("MODELDOWN"), "{text}");
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
            ("react", "⏳".to_string()),
            ("unreact", "⏳".to_string()),
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
