// Breaker tests for PR #138, round 2 (head 6675ca0). Throwaway; not for merge. Fixtures are made up.
use chrono::{DateTime, Local, NaiveDate, Utc};
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::calendar::Event;
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Slot};
use discord_inbox::ingest::{ingest_with, Filer, Inbound};
use discord_inbox::todo;
use discord_inbox::vaults::Vaults;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;
static N: AtomicUsize = AtomicUsize::new(0);
const BASE: u64 = 1_790_000_000;

fn dir(name: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("breaker138-r2-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn day(n: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, n).unwrap()
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn stamp(secs: u64) -> String {
    DateTime::<Utc>::from_timestamp(secs as i64, 0).unwrap().to_rfc3339()
}

// Stages the night marker, then sets its mtime (what a sync tool, a backup copy or `touch` would do).
fn stage_with_mtime(inbox: &Path, day: &str, mtime: SystemTime) {
    daily::stage(inbox, Slot::Night, day).unwrap();
    let f = fs::OpenOptions::new().write(true).open(daily::marker_path(inbox, Slot::Night, day)).unwrap();
    f.set_modified(mtime).unwrap();
}

fn ev(calendar: &str, summary: &str, date: &str) -> Event {
    Event { uid: "u".into(), calendar: calendar.into(), date: date.into(), time: Some("09:00".into()), summary: summary.into() }
}

struct Canned {
    reply: String,
    calls: AtomicUsize,
}
impl Chat for Canned {
    fn ask(&self, _s: &str, _u: &str) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.reply.clone())
    }
}
fn canned(reply: &str) -> Arc<Canned> {
    Arc::new(Canned { reply: reply.to_string(), calls: AtomicUsize::new(0) })
}

struct Down {
    calls: AtomicUsize,
}
impl Chat for Down {
    fn ask(&self, _s: &str, _u: &str) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err("model down".to_string())
    }
}

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

struct Msg {
    id: u64,
    text: String,
    received: String,
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
        self.received.clone()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        Vec::new()
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

fn has_note(inbox: &Path) -> bool {
    fs::read_dir(inbox).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().ends_with(".md"))
}

// ---- 1. Night gate: received time vs marker mtime ----

// Clock skew: the Mac clock runs 2 s ahead of Discord. The marker is staged at local BASE+5 (true BASE+3); a real reply received at Discord BASE+4 is after the DM and must count.

#[test]
#[ignore = "follow-up: see review round 2"]
fn r2_gate_clock_skew_reply_is_not_a_replay() {
    let inbox = dir("skew");
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    stage_with_mtime(&inbox, &today, at(BASE + 5));
    assert!(daily::after_send(&inbox, &today, &stamp(BASE + 4)), "a reply 1 s after the night DM was treated as a replay");
}

// The marker is touched (sync tool, backup copy without -p, `touch`) after a real reply arrived. The reply is then older than the marker and is filed as a note: the check-in is lost.
#[test]
#[ignore = "follow-up: see review round 2"]
fn r2_gate_marker_touched_after_reply_loses_checkin() {
    let inbox = dir("touched");
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    stage_with_mtime(&inbox, &today, at(BASE + 600));
    assert!(daily::after_send(&inbox, &today, &stamp(BASE + 10)), "a real reply (10 s after staging) is treated as a replay because the marker mtime moved to BASE+600");
}

// ---- 2. Model-error fallback: does b9_ really reach it? ----

// The b9_ scenario is not a red test. Its stamp (2026-10-02) predates today's marker, so after_send is false and answer() never runs. See the LOG.

// ---- 3. Empty DM skip: zero-width space is not whitespace to trim() ----

// A DM whose only content is U+200B goes to the check-in model and, with a model that says checkin:true, is consumed (marker answered, no Inbox note). It is not an empty DM to filter out.
#[tokio::test]
async fn r2_zero_width_dm_is_not_a_checkin() {
    let root = dir("zwsp");
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    daily::stage(&inbox, Slot::Night, &today).unwrap();
    let chat = canned(r#"{"checkin": true, "done": [], "tomorrow": []}"#);
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id: 902, text: "\u{200b}".into(), received: "2099-01-01T00:00:00Z".into(), replies: replies.clone() };
    let filer = Filer { chat: chat.clone(), vaults: Vaults::new(root.clone()), last_notice: Default::default(), window: Default::default() };
    let _ = ingest_with(&allow(), &inbox, &m, &filer).await;
    assert!(has_note(&inbox), "a zero-width-only DM was consumed as a check-in and filed nowhere; calls: {}, replies: {:?}", chat.calls.load(Ordering::SeqCst), replies.lock().unwrap());
}

// ---- 4. Whole-word exam match ----

// "Mid-term" is the hyphenated spelling; the split on non-alphanumerics yields "mid" and "term", so the exam day loses its calculator line.
#[test]
#[ignore = "follow-up: see review round 2"]
fn r2_is_exam_mid_hyphen_term() {
    assert!(daily::is_exam(&ev("Work", "Mid-term 2", "2026-10-29")), "\"Mid-term 2\" is not an exam");
}

// Digits attached to the word: "Midterm1" and "Exam1" are one token after the split.
#[test]
#[ignore = "follow-up: see review round 2"]
fn r2_is_exam_digit_suffix() {
    assert!(daily::is_exam(&ev("Work", "Midterm1 review", "2026-10-29")), "\"Midterm1\" is not an exam");
}

// ---- 5. Deadlines: calendar name case ----

// Lectures and Office Hours are excluded by an exact, case-sensitive suffix match. "Office hours" typed by hand is not excluded, so every office-hour slot shows up under Due soon.
#[test]
#[ignore = "follow-up: see review round 2"]
fn r2_is_deadline_office_hours_case() {
    assert!(!daily::is_deadline(&ev("Office hours", "Ask about lab 3", "2026-10-29")), "an office-hours slot counts as a deadline");
}

// ---- 6. Umbrella guard and the reply limits ----

fn facts() -> String {
    "Weather (Stony Brook): 60°F now, high 65 / low 50, sunny\nWear: jacket\nToday:\n- 09:00 MATH 101 (Work)".to_string()
}

#[test]
fn r2_voice_raincoat_passes_guard() {
    let f = facts();
    let out = daily::voice(canned("Morning! Grab a raincoat before you head out. MATH 101 at 09:00.").as_ref(), Slot::Morning, &f);
    assert_eq!(out, f, "a rain-gear reply got past the umbrella guard");
}

#[test]
fn r2_voice_umbrella_emoji_passes_guard() {
    let f = facts();
    let out = daily::voice(canned("Morning \u{2614} Heavy day ahead. MATH 101 at 09:00.").as_ref(), Slot::Morning, &f);
    assert_eq!(out, f, "the umbrella emoji got past the guard");
}

// The prompt says "Under 1500 characters" and nothing in code enforces it: a 1600-character reply goes out unchanged.
#[test]
fn r2_voice_over_1500_chars_is_not_passed_through() {
    let f = facts();
    let long = format!("Morning. {}", "Keep the lab notes in order today. ".repeat(45));
    assert!(long.chars().count() > 1500);
    let out = daily::voice(canned(&long).as_ref(), Slot::Morning, &f);
    assert!(out.chars().count() <= 1500 || out == f, "a {} character reply went out; the limit is prompt-only", out.chars().count());
}
// ---- 7. Symlinks and checkboxes in ! Todo ----

// A symlinked day file is read into the brief. The coder skips symlinked notes ("its target is not ours to read"), but not day files.
#[test]
fn r2_symlinked_day_file_is_not_read() {
    let root = dir("symday");
    let todo_dir = root.join("! Todo");
    fs::create_dir_all(&todo_dir).unwrap();
    let outside = root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("Secret.md"), "- [ ] outside secret\n").unwrap();
    std::os::unix::fs::symlink(outside.join("Secret.md"), todo_dir.join("2026-10-28.md")).unwrap();
    let items = todo::day_items(&todo_dir, day(28));
    assert!(items.is_empty(), "a symlinked day file was read: {:?}", items.iter().map(|i| &i.text).collect::<Vec<_>>());
}

// A bare "- [ ]" (an empty checkbox, as an Obsidian template makes) is not stripped by "- [ ] " (needs the trailing space) and comes out as an open item named "[ ]".
#[test]
fn r2_bare_checkbox_is_not_an_item() {
    let root = dir("bare");
    let todo_dir = root.join("! Todo");
    fs::create_dir_all(&todo_dir).unwrap();
    fs::write(todo_dir.join("2026-10-28.md"), "- [ ]\n- [ ] real item\n").unwrap();
    let texts: Vec<String> = todo::day_items(&todo_dir, day(28)).into_iter().map(|i| i.text).collect();
    assert_eq!(texts, vec!["real item".to_string()]);
}

// A UTF-8 BOM before a body line without front matter is kept in the text and the heading is not recognised as a heading.
#[test]
fn r2_bom_before_heading_is_not_an_item() {
    let root = dir("bom");
    let todo_dir = root.join("! Todo");
    fs::create_dir_all(&todo_dir).unwrap();
    fs::write(todo_dir.join("2026-10-28.md"), "\u{feff}# Plan\n- [ ] buy milk\n").unwrap();
    let texts: Vec<String> = todo::day_items(&todo_dir, day(28)).into_iter().map(|i| i.text).collect();
    assert_eq!(texts, vec!["buy milk".to_string()], "got {:?}", texts);
}

// A bullet-wrapped strike "- ~~foo~~" is struck text, but is_open only excludes lines that start with "~~", so it stays an open item.
#[test]
fn r2_bullet_wrapped_strike_is_not_open() {
    let root = dir("bulletstrike");
    let todo_dir = root.join("! Todo");
    fs::create_dir_all(&todo_dir).unwrap();
    fs::write(todo_dir.join("2026-10-28.md"), "- ~~foo~~\n").unwrap();
    let texts: Vec<String> = todo::day_items(&todo_dir, day(28)).into_iter().map(|i| i.text).collect();
    assert!(texts.is_empty(), "struck item listed as open: {:?}", texts);
}

// ---- 8. Check-in log: model text with a newline ----

// apply() sanitises the carried texts for the day file, but passes them raw to log_checkin. A model item with a newline writes a fake heading into the check-in log.
#[test]
fn r2_checkin_log_newline_injection() {
    let root = dir("lognl");
    let todo_dir = root.join("! Todo");
    fs::create_dir_all(&todo_dir).unwrap();
    todo::log_checkin(&todo_dir, day(28), "ok", &[], &["call Bob\n## 2099-01-01".to_string()]).unwrap();
    let log = fs::read_to_string(todo_dir.join(todo::CHECKIN_LOG)).unwrap();
    assert!(!log.lines().any(|l| l.starts_with("## 2099-01-01")), "a fake check-in heading was written:\n{log}");
}

// ---- 9. Model JSON: integral floats ----

// done: [1.0] (a float the model may emit) is dropped, so the item is not struck.
#[test]
#[ignore = "follow-up: see review round 2"]
fn r2_parse_answer_float_item_number() {
    let a = daily::parse_answer(r#"{"checkin": true, "done": [1.0], "tomorrow": []}"#, 3).unwrap();
    assert_eq!(a.done, vec![1usize]);
}

// ---- 10. Tmp-file race under concurrency (passing check kept for the log) ----

#[test]
fn r2_concurrent_carry_keeps_every_item() {
    let root = dir("carryrace");
    let todo_dir = root.join("! Todo");
    let handles: Vec<_> = (0..32)
        .map(|i| {
            let t = todo_dir.clone();
            std::thread::spawn(move || todo::carry(&t, day(29), &[format!("task number {i}")]).unwrap())
        })
        .collect();
    let added: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
    let body = fs::read_to_string(todo_dir.join("2026-10-29.md")).unwrap();
    assert_eq!(added, 32);
    assert_eq!(body.lines().filter(|l| l.starts_with("- task number")).count(), 32);
}
