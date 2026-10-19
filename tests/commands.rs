// Stormy's typed DM commands (#92): `/commands` lists the basics, `/clear` posts blank lines and forgets the last few turns.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::commands::{self, Slash, CLEAR_LINES, COMMANDS_TEXT};
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome, Window};
use discord_inbox::insight::DISCORD_MAX;
use discord_inbox::vaults::Vaults;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn at(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc)
}

#[test]
fn parse_matches_exact_commands_only() {
    assert_eq!(commands::parse("/commands"), Some(Slash::Commands));
    assert_eq!(commands::parse(" /Commands \n"), Some(Slash::Commands));
    assert_eq!(commands::parse("/CLEAR"), Some(Slash::Clear));
    for no in ["/clear please", "clear", "/clearall", "what does /clear do", "", "/"] {
        assert_eq!(commands::parse(no), None, "{no:?} must not be a command");
    }
}

#[test]
fn clear_text_is_tall_and_never_blank() {
    let text = commands::clear_text();
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines.len(), CLEAR_LINES);
    assert!(lines.iter().all(|l| *l == "_ _"));
    assert!(!text.trim().is_empty());
    assert!(text.chars().count() <= DISCORD_MAX);
}

#[test]
fn commands_text_lists_the_basics() {
    for want in ["/commands", "/clear", "restock", "out of"] {
        assert!(COMMANDS_TEXT.contains(want), "missing {want}");
    }
    assert!(COMMANDS_TEXT.chars().count() <= DISCORD_MAX);
}

#[test]
fn forget_drops_older_turns_and_refuses_late_pushes() {
    let w = Window::default();
    w.push(5, "2026-10-14T16:01:00Z", "a", "b");
    w.push(6, "2026-10-14T16:01:00Z", "c", "d");
    w.forget(5, "2026-10-14T16:02:00Z");
    // A reply to a turn sent before the /clear lands after it: its memory is refused.
    w.push(5, "2026-10-14T16:01:30Z", "late", "late");
    assert!(w.recent_at(5, at("2026-10-14T16:10:00Z")).is_empty());
    // A turn after the /clear is kept.
    w.push(5, "2026-10-14T16:03:00Z", "new", "new");
    assert_eq!(w.recent_at(5, at("2026-10-14T16:10:00Z")), vec![("new".to_string(), "new".to_string())]);
    // Other channels are untouched.
    assert_eq!(w.recent_at(6, at("2026-10-14T16:10:00Z")).len(), 1);
}

#[test]
fn forget_invariant_holds_for_generated_sequences() {
    // Fixed-seed LCG: no external RNG crate. Stamps sit in one 30-minute band so every kept turn is inside the window.
    let mut seed: u64 = 0x92;
    let mut next = move |n: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let now = at("2026-10-14T16:30:00Z");
    let mut survivors_checked = 0usize;
    for _ in 0..500 {
        let shared = Window::default();
        let twin = Window::default();
        let mut pushed: [std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>; 3] = Default::default();
        let mut forgot: [Option<chrono::DateTime<chrono::Utc>>; 3] = [None; 3];
        for op in 0..30u64 {
            let ch = 1 + next(2);
            let m = next(30);
            let s = next(60);
            let stamp = if next(2) == 0 {
                format!("2026-10-14T16:{m:02}:{s:02}Z")
            } else {
                format!("2026-10-14T12:{m:02}:{s:02}-04:00")
            };
            let when = at(&stamp);
            if next(4) == 0 {
                shared.forget(ch, &stamp);
                let c = ch as usize;
                forgot[c] = Some(forgot[c].map_or(when, |f| f.max(when)));
                if ch == 2 {
                    twin.forget(2, &stamp);
                }
            } else {
                let him = format!("him{op}");
                shared.push(ch, &stamp, &him, "her");
                pushed[ch as usize].insert(him.clone(), when);
                if ch == 2 {
                    twin.push(2, &stamp, &him, "her");
                }
            }
            for c in [1u64, 2] {
                for (him, _) in shared.recent_at(c, now) {
                    survivors_checked += 1;
                    let stamp = pushed[c as usize][&him];
                    // (a) nothing at or before this channel's last forget survives.
                    if let Some(f) = forgot[c as usize] {
                        assert!(stamp > f, "{him} at {stamp} survived forget {f} on channel {c}");
                    }
                }
            }
            // (b) channel isolation: channel 2 matches a twin that only saw channel 2's ops.
            assert_eq!(shared.recent_at(2, now), twin.recent_at(2, now));
        }
    }
    assert!(survivors_checked > 1000, "the property must actually see kept turns, saw {survivors_checked}");
}

static N: AtomicUsize = AtomicUsize::new(0);
const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn setup(name: &str) -> (PathBuf, PathBuf) {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("di-commands-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    (inbox, root)
}

struct Never;
impl Chat for Never {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        panic!("the model must not be asked for a slash command");
    }
}

fn filer(root: &Path) -> Filer {
    Filer { chat: Arc::new(Never), vaults: Vaults::new(root.to_path_buf()), last_notice: Default::default(), window: Default::default() }
}

struct Msg {
    id: u64,
    text: String,
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
        self.replies.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

#[tokio::test]
async fn commands_dm_replies_list_without_the_model() {
    let (inbox, root) = setup("list");
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id: 92001, text: "/commands".into(), replies: replies.clone() };
    let out = ingest_with(&allow(), &inbox, &m, &filer(&root)).await;
    assert_eq!(out, Outcome::Saved);
    assert_eq!(*replies.lock().unwrap(), vec![COMMANDS_TEXT.to_string()]);
}

#[tokio::test]
async fn clear_dm_posts_blank_block_and_forgets() {
    let (inbox, root) = setup("clear");
    let f = filer(&root);
    for (i, t) in ["14:50", "14:55", "14:59"].iter().enumerate() {
        f.window.push(CH, &format!("2026-10-02T{t}:00Z"), &format!("him{i}"), "her");
    }
    f.window.push(9, "2026-10-02T14:59:00Z", "other", "her");
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id: 92002, text: "/clear".into(), replies: replies.clone() };
    let out = ingest_with(&allow(), &inbox, &m, &f).await;
    assert_eq!(out, Outcome::Saved);
    assert_eq!(*replies.lock().unwrap(), vec![commands::clear_text()]);
    assert!(f.window.recent_at(CH, at("2026-10-02T15:05:00Z")).is_empty());
    assert_eq!(f.window.recent_at(9, at("2026-10-02T15:05:00Z")).len(), 1);
}

#[tokio::test]
async fn clear_dm_replayed_is_skipped() {
    let (inbox, root) = setup("replay");
    let f = filer(&root);
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id: 92003, text: "/clear".into(), replies: replies.clone() };
    assert_eq!(ingest_with(&allow(), &inbox, &m, &f).await, Outcome::Saved);
    assert_eq!(ingest_with(&allow(), &inbox, &m, &f).await, Outcome::Skipped);
    assert_eq!(replies.lock().unwrap().len(), 1);
}

#[allow(dead_code)]
fn unused(_: Slash) {}
