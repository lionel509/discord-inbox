//! Breaker: stale fetched-message snapshots against the reaction marks. Fake IDs only.
use discord_inbox::allowlist;
use discord_inbox::dump;
use discord_inbox::ingest::{ingest, Inbound, Outcome};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;
const ID: u64 = 42;
const RECEIVED: &str = "2026-10-14T09:00:00Z";

struct World {
    inbox: PathBuf,
    cdn_down: AtomicBool,
    marks: Mutex<HashMap<u64, Vec<String>>>,
}

/// A message object. `snapshot` is what its own_reactions returns, the way a fetched
/// Discord message carries reactions as of the fetch, not live.
struct Snap {
    w: Arc<World>,
    snapshot: Vec<String>,
}

impl Inbound for Snap {
    fn author_id(&self) -> u64 { MAIN }
    fn msg_id(&self) -> u64 { ID }
    fn channel_id(&self) -> u64 { CH }
    fn received(&self) -> String { RECEIVED.to_string() }
    fn content(&self) -> String { "hi".to_string() }
    fn attachment_names(&self) -> Vec<String> { vec!["bad-scan.pdf".to_string()] }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        if self.w.cdn_down.load(Ordering::SeqCst) {
            return Err("cdn 503".to_string());
        }
        Ok(b"%PDF-1.4 fake".to_vec())
    }
    async fn react(&self, emoji: &str) -> Result<(), String> {
        let mut m = self.w.marks.lock().unwrap();
        let v = m.entry(ID).or_default();
        if !v.iter().any(|e| e == emoji) {
            v.push(emoji.to_string());
        }
        Ok(())
    }
    async fn unreact(&self, emoji: &str) -> Result<(), String> {
        if let Some(v) = self.w.marks.lock().unwrap().get_mut(&ID) {
            v.retain(|e| e != emoji);
        }
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> { self.snapshot.clone() }
    async fn reply(&self, _text: &str) -> Result<(), String> { Ok(()) }
}

fn world(name: &str) -> (Arc<World>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("di-breaker-marks-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // Keep OCR and the local model out of it: a missing helper just yields no text.
    std::env::set_var("DISCORD_INBOX_OCR", "/usr/bin/false");
    std::env::set_var("DISCORD_INBOX_INSIGHT", "/usr/bin/false");
    let inbox = dir.join("Inbox");
    let w = Arc::new(World {
        inbox: inbox.clone(),
        cdn_down: AtomicBool::new(false),
        marks: Mutex::new(HashMap::new()),
    });
    (w, inbox)
}

fn marks(w: &World) -> Vec<String> {
    w.marks.lock().unwrap().get(&ID).cloned().unwrap_or_default()
}

/// Live ingest hits a CDN failure: ⏳ goes on. A catch-up copy fetched before that
/// still has no reactions; the CDN recovers and the same message completes.
/// The mark must end as ✅ alone (README: "a message never shows both").
#[tokio::test]
async fn breaker_stale_fetch_pending_then_saved_leaves_both_marks() {
    let (w, inbox) = world("pending-stale");
    let allow = allowlist::parse(&format!("{MAIN}\n")).unwrap();

    let fetched_before_live = Snap { w: w.clone(), snapshot: vec![] };
    let live = Snap { w: w.clone(), snapshot: vec![] };

    w.cdn_down.store(true, Ordering::SeqCst);
    assert_eq!(ingest(&allow, &inbox, &live).await, Outcome::Pending);
    assert_eq!(marks(&w), vec!["⏳".to_string()]);

    w.cdn_down.store(false, Ordering::SeqCst);
    let out = ingest(&allow, &inbox, &fetched_before_live).await;
    assert_eq!(out, Outcome::Saved);
    assert!(!dump::in_retry(&inbox, ID));
    assert_eq!(marks(&w), vec!["✅".to_string()], "⏳ left behind next to ✅");
}
