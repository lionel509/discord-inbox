//! Behaviour of `ingest` and `catch_up` against fakes. Fake IDs only.
use discord_inbox::allowlist;
use discord_inbox::allowlist::Allowlist;
use discord_inbox::dump::{
    self, add_retry, in_retry, read_cursor, retry_entries, sha256_hex, write_cursor,
};
use discord_inbox::ingest::{catch_up, ingest, History, Inbound, Outcome, PAGE};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

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

/// A fresh scratch dir; returns its `Inbox` path, which is NOT created.
fn scratch(name: &str) -> PathBuf {
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
    /// msg id -> (downloads, reacts)
    calls: Mutex<HashMap<u64, (usize, usize)>>,
    note_at_react: Mutex<HashMap<u64, bool>>,
}

impl World {
    fn new(inbox: &Path) -> Arc<World> {
        Arc::new(World {
            inbox: inbox.to_path_buf(),
            cdn_down: AtomicBool::new(false),
            react_fails: AtomicBool::new(false),
            calls: Mutex::new(HashMap::new()),
            note_at_react: Mutex::new(HashMap::new()),
        })
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
    async fn react(&self) -> Result<(), String> {
        self.w.add_react(self.spec.id);
        let at = note(&self.w.inbox, self.spec.id).is_file();
        self.w
            .note_at_react
            .lock()
            .unwrap()
            .insert(self.spec.id, at);
        if self.w.react_fails.load(Ordering::SeqCst) {
            return Err("react 500".to_string());
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
    assert_eq!(w.reacts(100), 0);
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
    assert_eq!(w.total_reacts(), 1);

    w.cdn_down.store(false, Ordering::SeqCst);
    catch_up(&allow(), &inbox, &h).await;
    assert!(retry_entries(&inbox).is_empty());
    let second = fs::read_to_string(note(&inbox, 100)).unwrap();
    assert!(!second.contains("pending"), "{second}");
    assert!(att(&inbox, 100, 1, "bad.jpg").is_file());
    assert_eq!(w.reacts(100), 1);
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
    assert_eq!(w.total_reacts(), 2);

    fs::remove_dir(note(&inbox, 200)).unwrap();
    catch_up(&allow(), &inbox, &h).await;
    assert!(note(&inbox, 200).is_file());
    assert_eq!(w.reacts(200), 1);
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
        // I3: a react means the note is complete.
        for s in &msgs {
            if w.reacts(s.id) > 0 {
                assert!(
                    note(&inbox, s.id).is_file(),
                    "round {round}: I3 no note for reacted {}",
                    s.id
                );
                assert!(
                    !in_retry(&inbox, s.id),
                    "round {round}: I3 retry left for reacted {}",
                    s.id
                );
                let text = fs::read_to_string(note(&inbox, s.id)).unwrap();
                assert!(
                    !text.contains("pending"),
                    "round {round}: I3 pending note reacted {}",
                    s.id
                );
            }
        }
    }
}
