//! U4: Pip's catch-up over replayed DMs, using fake Discord history (as tests/ingest.rs does). Scratch dirs are `…/Private/Nutrition`.
use discord_inbox::allowlist;
use discord_inbox::dump;
use discord_inbox::ingest::{catch_up_each, History, Inbound, Outcome};
use discord_inbox::daylog;
use discord_inbox::pip::{self, Config, PipHandler};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MAIN: u64 = 100000000000000001;
const STRANGER: u64 = 100000000000000009;
const MAIN_CH: u64 = 5001;

#[derive(Clone)]
struct Spec {
    id: u64,
    author: u64,
    received: &'static str,
    text: &'static str,
    files: Vec<(&'static str, Vec<u8>)>,
}

struct FakeMsg {
    spec: Spec,
    live: bool,
    downloads: Arc<AtomicUsize>,
}

impl Inbound for FakeMsg {
    fn author_id(&self) -> u64 {
        self.spec.author
    }
    fn msg_id(&self) -> u64 {
        self.spec.id
    }
    fn channel_id(&self) -> u64 {
        MAIN_CH
    }
    fn received(&self) -> String {
        self.spec.received.to_string()
    }
    fn content(&self) -> String {
        self.spec.text.to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.spec.files.iter().map(|f| f.0.to_string()).collect()
    }
    fn live(&self) -> bool {
        self.live
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        let (name, bytes) = &self.spec.files[i];
        if name.starts_with("bad") {
            return Err("cdn 503".to_string());
        }
        Ok(bytes.clone())
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
    async fn reply(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

struct FakeHistory {
    msgs: Mutex<Vec<Spec>>,
}

impl History for FakeHistory {
    type Msg = FakeMsg;
    async fn dm_channel(&self, user_id: u64) -> Result<u64, String> {
        if user_id == MAIN || user_id == STRANGER {
            Ok(MAIN_CH)
        } else {
            Err("no dm channel".to_string())
        }
    }
    async fn page(&self, channel: u64, after: Option<u64>) -> Result<Vec<FakeMsg>, String> {
        if channel != MAIN_CH {
            return Ok(vec![]);
        }
        Ok(self
            .msgs
            .lock()
            .unwrap()
            .iter()
            .filter(|s| after.is_none_or(|a| s.id > a))
            .map(|s| FakeMsg { spec: s.clone(), live: false, downloads: Arc::new(AtomicUsize::new(0)) })
            .collect())
    }
    async fn fetch(&self, _channel: u64, msg_id: u64) -> Result<FakeMsg, String> {
        self.msgs
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == msg_id)
            .map(|s| FakeMsg { spec: s.clone(), live: false, downloads: Arc::new(AtomicUsize::new(0)) })
            .ok_or_else(|| "gone".to_string())
    }
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-ingest-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn cfg(dir: &Path) -> Config {
    Config { nutrition_dir: dir.to_path_buf(), recipes_dir: None, foods_dir: None, low_kcal: 1800, kcal_target: 2400, model: None, model_timeout: std::time::Duration::from_secs(90) }
}

fn batch() -> Vec<Spec> {
    vec![
        Spec { id: 1001, author: MAIN, received: "2026-10-25T12:00:00-04:00", text: "100g salmon", files: vec![] },
        Spec { id: 1002, author: MAIN, received: "2026-10-25T19:00:00-04:00", text: "1 tbsp oil", files: vec![] },
        Spec { id: 1003, author: MAIN, received: "2026-10-26T08:00:00-04:00", text: "cabbage", files: vec![("IMG_1.jpg", b"abc".to_vec())] },
        Spec { id: 1004, author: STRANGER, received: "2026-10-26T09:00:00-04:00", text: "100g salmon", files: vec![] },
        Spec { id: 1005, author: MAIN, received: "2026-10-26T12:00:00-04:00", text: "100g salmon", files: vec![] },
    ]
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| {
                let p = e.unwrap().path();
                (p.file_name().unwrap().to_string_lossy().into_owned(), fs::read(&p).unwrap_or_default())
            })
            .collect(),
        Err(_) => vec![],
    };
    v.sort();
    v
}

#[tokio::test]
async fn catch_up_logs_the_allowlisted_dms_by_day_and_moves_the_cursor() {
    let dir = scratch("batch");
    let (a, c) = (allow(), cfg(&dir));
    let h = FakeHistory { msgs: Mutex::new(batch()) };
    catch_up_each(&a, &dir, &h, &PipHandler { allow: &a, cfg: &c }).await;

    let d25 = fs::read_to_string(daylog::note_path(&dir, "2026-10-25")).expect("day one note");
    let d26 = fs::read_to_string(daylog::note_path(&dir, "2026-10-26")).expect("day two note");
    assert!(d25.contains("msg 1001") && d25.contains("msg 1002"), "{d25}");
    assert!(d26.contains("msg 1003") && d26.contains("msg 1005"), "{d26}");
    assert!(!d26.contains("msg 1004"), "a stranger's DM must not be logged");

    let jpgs = snapshot(&dir).iter().filter(|(n, _)| n.ends_with(".jpg")).count();
    assert_eq!(jpgs, 1, "one photo saved beside the notes");

    assert_eq!(dump::read_cursor(&dir, MAIN_CH), Some(1005), "cursor is the newest handled DM");
}

#[tokio::test]
async fn replaying_the_same_history_adds_nothing() {
    let dir = scratch("replay");
    let (a, c) = (allow(), cfg(&dir));
    let h = FakeHistory { msgs: Mutex::new(batch()) };
    catch_up_each(&a, &dir, &h, &PipHandler { allow: &a, cfg: &c }).await;
    let before = snapshot(&dir);
    catch_up_each(&a, &dir, &h, &PipHandler { allow: &a, cfg: &c }).await;
    assert_eq!(before, snapshot(&dir), "a replayed DM must not add a block (I1)");
    assert_eq!(dump::read_cursor(&dir, MAIN_CH), Some(1005));
}

#[tokio::test]
async fn a_failed_photo_download_writes_nothing_and_holds_the_cursor() {
    let dir = scratch("failed");
    let (a, c) = (allow(), cfg(&dir));
    let msgs = vec![
        Spec { id: 2001, author: MAIN, received: "2026-10-25T12:00:00-04:00", text: "cabbage", files: vec![("bad.jpg", b"x".to_vec())] },
        Spec { id: 2002, author: MAIN, received: "2026-10-25T13:00:00-04:00", text: "100g salmon", files: vec![] },
    ];
    let h = FakeHistory { msgs: Mutex::new(msgs) };
    catch_up_each(&a, &dir, &h, &PipHandler { allow: &a, cfg: &c }).await;
    let text = fs::read_to_string(daylog::note_path(&dir, "2026-10-25")).unwrap_or_default();
    assert!(!text.contains("msg 2001"), "failed message must not be written: {text}");
    assert_eq!(dump::read_cursor(&dir, MAIN_CH), None, "cursor must stop before the failure");
}

#[tokio::test]
async fn a_live_message_with_no_text_and_no_files_is_ignored() {
    let dir = scratch("empty");
    let (a, c) = (allow(), cfg(&dir));
    let m = FakeMsg {
        spec: Spec { id: 3001, author: MAIN, received: "2026-10-25T12:00:00-04:00", text: "   ", files: vec![] },
        live: true,
        downloads: Arc::new(AtomicUsize::new(0)),
    };
    let out = pip::handle(&a, &c, &m).await;
    assert!(matches!(out, Outcome::Ignored), "{out:?}");
    assert!(snapshot(&dir).is_empty(), "nothing written");
}

#[tokio::test]
async fn replayed_photo_dm_is_not_downloaded_again() {
    let dir = scratch("replay-photo");
    let (a, c) = (allow(), cfg(&dir));
    let downloads = Arc::new(AtomicUsize::new(0));
    let m = FakeMsg {
        spec: Spec { id: 4001, author: MAIN, received: "2026-10-25T12:00:00-04:00", text: "cabbage", files: vec![("IMG_4.jpg", b"abc".to_vec())] },
        live: false,
        downloads: downloads.clone(),
    };
    let first = pip::handle(&a, &c, &m).await;
    assert!(matches!(first, Outcome::Saved), "first delivery: {first:?}");
    assert_eq!(downloads.load(Ordering::SeqCst), 1, "first delivery downloads once");

    let second = pip::handle(&a, &c, &m).await;
    assert!(matches!(second, Outcome::Skipped), "replay: {second:?}");
    assert_eq!(downloads.load(Ordering::SeqCst), 1, "a replayed photo DM must not download again (logged() gates the download)");
}
