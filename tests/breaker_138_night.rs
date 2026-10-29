// Breaker tests for PR #138 (head 64e75ad), night check-in. Throwaway; not for merge. Folders are made-up fixtures.
use chrono::{Local, NaiveDate};
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Answer, Deps, Slot};
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome};
use discord_inbox::todo;
use discord_inbox::vaults::Vaults;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once};

const MAIN: u64 = 100000000000000001;
const CH: u64 = 7001;
static N: AtomicUsize = AtomicUsize::new(0);
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

fn guard() -> MutexGuard<'static, ()> {
    INIT.call_once(|| {
        let tmp = std::env::temp_dir().join(format!("di-breaker138-{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        let calendars = tmp.join("calendars");
        fs::write(&calendars, "MATH 101 - Work\n").unwrap();
        std::env::set_var("DISCORD_INBOX_CALENDARS_FILE", &calendars);
        std::env::set_var("DISCORD_INBOX_OSASCRIPT", PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/calendar_stub.sh"));
        std::env::set_var("DISCORD_INBOX_CALENDAR_DB", PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calendar.sqlitedb"));
        std::env::set_var("DISCORD_INBOX_CALENDAR_LOG", tmp.join("stub.log"));
    });
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn allow() -> Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn setup(name: &str) -> (PathBuf, PathBuf) {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("di-breaker138-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    (inbox, root)
}

/// Answers from a script, in order.
struct Seq(Mutex<VecDeque<String>>);
impl Seq {
    fn new(replies: &[&str]) -> Arc<Seq> {
        Arc::new(Seq(Mutex::new(replies.iter().map(|s| s.to_string()).collect())))
    }
}
impl Chat for Seq {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        match self.0.lock().unwrap().pop_front() {
            Some(text) => Ok(text),
            None => panic!("the model was asked more times than scripted"),
        }
    }
}

/// The model is down (quota or network).
struct Down;
impl Chat for Down {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Err("429 rate limited".to_string())
    }
}

fn filer(root: &Path, chat: Arc<dyn Chat>) -> Filer {
    Filer { chat, vaults: Vaults::new(root.to_path_buf()), last_notice: Default::default(), window: Default::default() }
}

struct Msg {
    id: u64,
    text: String,
    // None: the DM arrives now (after the night marker). Some: a fixed stamp, for a message from before it.
    received: Option<String>,
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
        self.received.clone().unwrap_or_else(|| discord_inbox::when::local_stamp(chrono::Utc::now()))
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

async fn say(inbox: &Path, root: &Path, chat: Arc<dyn Chat>, id: u64, text: &str, received: Option<&str>) -> (Outcome, Vec<String>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id, text: text.to_string(), received: received.map(str::to_string), replies: replies.clone() };
    let out = ingest_with(&allow(), inbox, &m, &filer(root, chat)).await;
    let all = replies.lock().unwrap().clone();
    (out, all)
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn write_day(root: &Path, day: NaiveDate, body: &str) -> PathBuf {
    let dir = root.join(todo::DIR);
    fs::create_dir_all(&dir).unwrap();
    fs::write(todo::day_file(&dir, day), body).unwrap();
    dir
}

fn has_note(inbox: &Path) -> bool {
    fs::read_dir(inbox).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().ends_with(".md"))
}

fn deps(dir: &Path, chat: Arc<dyn Chat>) -> Deps {
    Deps { todo_dir: dir.to_path_buf(), cal: None, chat }
}

const RECEIVED: &str = "2026-10-02T15:00:00Z"; // a message from before the night DM

// BREAK 1: a normal note sent during the awaiting window is swallowed when the model call fails.
#[tokio::test]
async fn b1_normal_note_filed_when_model_is_down() {
    let _g = guard();
    let (inbox, root) = setup("down");
    let day = ymd(today());
    write_day(&root, today(), "- finish lab report\n");
    daily::stage(&inbox, Slot::Night, &day).unwrap();
    let (out, replies) = say(&inbox, &root, Arc::new(Down), 11, "remind me to email the bursar tomorrow", None).await;
    assert!(has_note(&inbox), "normal note must reach Inbox; outcome {out:?}, replies {replies:?}");
}

// BREAK 1b: same, when the model answers with non-JSON.
#[tokio::test]
async fn b1b_normal_note_filed_when_model_reply_is_not_json() {
    let _g = guard();
    let (inbox, root) = setup("notjson");
    let day = ymd(today());
    write_day(&root, today(), "- finish lab report\n");
    daily::stage(&inbox, Slot::Night, &day).unwrap();
    let chat = Seq::new(&["Sure, happy to help!", "ok", "ok"]);
    let (out, replies) = say(&inbox, &root, chat, 12, "remind me to email the bursar tomorrow", None).await;
    assert!(has_note(&inbox), "normal note must reach Inbox; outcome {out:?}, replies {replies:?}");
}

// BREAK 2: an empty reply is trusted as a check-in and strikes a to-do.
#[tokio::test]
async fn b2_empty_reply_does_not_strike_a_todo() {
    let _g = guard();
    let (inbox, root) = setup("empty");
    let day = today();
    let dir = write_day(&root, day, "- finish lab report\n");
    daily::stage(&inbox, Slot::Night, &ymd(day)).unwrap();
    let chat = Seq::new(&[r#"{"checkin":true,"done":[1],"tomorrow":[]}"#, "ok", "ok"]);
    let (out, replies) = say(&inbox, &root, chat, 13, "", None).await;
    let body = fs::read_to_string(todo::day_file(&dir, day)).unwrap();
    assert!(!body.contains("~~"), "an empty DM must not strike a to-do; outcome {out:?}, replies {replies:?}, file {body:?}");
}

// BREAK 3: a checked box in a to-do file counts as open.
#[test]
fn b3_checked_box_lines_are_not_open() {
    let _g = guard();
    let (_inbox, root) = setup("boxes");
    let day = today();
    let dir = write_day(&root, day, "---\ntags:\n  - todo\n---\n- [x] shipped the lab\n- [X] also done\n- [ ] write essay\n* [ ] email prof\n");
    let texts: Vec<String> = todo::day_items(&dir, day).into_iter().map(|i| i.text).collect();
    assert_eq!(texts, vec!["write essay".to_string(), "email prof".to_string()], "checked boxes must not be open items");
}

// BREAK 3b: a checked box is carried to tomorrow by the night check-in.
#[test]
fn b3b_checked_box_is_not_carried() {
    let _g = guard();
    let (_inbox, root) = setup("boxcarry");
    let day = today();
    let dir = write_day(&root, day, "- [x] shipped the lab\n- [ ] write essay\n");
    let chat = Seq::new(&[r#"{"checkin":true,"done":[],"tomorrow":[]}"#]);
    let reply = daily::answer(&deps(&dir, chat), day, "nothing else done yet", None).unwrap().unwrap();
    let next = day.succ_opt().unwrap();
    let tomorrow = fs::read_to_string(todo::day_file(&dir, next)).unwrap_or_default();
    assert!(!tomorrow.contains("shipped the lab"), "finished item carried to tomorrow; reply {reply:?}, tomorrow file {tomorrow:?}");
}

// BREAK 4: the model rewords an unfinished item in `tomorrow`; the case-sensitive dedupe keeps both.
#[test]
fn b4_reworded_carry_duplicates_a_todo() {
    let _g = guard();
    let (_inbox, root) = setup("dup");
    let day = today();
    let dir = write_day(&root, day, "- finish lab report\n");
    let chat = Seq::new(&[r#"{"checkin":true,"done":[],"tomorrow":["Finish lab report"]}"#]);
    daily::answer(&deps(&dir, chat), day, "lab is not done", None).unwrap();
    let body = fs::read_to_string(todo::day_file(&dir, day.succ_opt().unwrap())).unwrap();
    let n = body.lines().filter(|l| l.to_lowercase().contains("finish lab report")).count();
    assert_eq!(n, 1, "one to-do must appear once tomorrow, file: {body:?}");
}

// BREAK 5: two apply calls with the same item list (a race or a lost marker write) double-strike the line.
#[test]
fn b5_repeated_apply_does_not_mangle_a_strike() {
    let _g = guard();
    let (_inbox, root) = setup("twice");
    let day = today();
    let dir = write_day(&root, day, "- finish lab report\n");
    let items = todo::day_items(&dir, day);
    let a = Answer { checkin: true, done: vec![1], tomorrow: vec![] };
    daily::apply(&dir, day, &items, &a).unwrap();
    daily::apply(&dir, day, &items, &a).unwrap();
    let body = fs::read_to_string(todo::day_file(&dir, day)).unwrap();
    assert!(!body.contains("~~~~"), "strike mangled by a second apply: {body:?}");
}

// BREAK 6: concurrent check-in logs (two replies at once, different message ids) share one tmp file name.
#[test]
fn b6_concurrent_checkin_logs_all_succeed() {
    let _g = guard();
    let (_inbox, root) = setup("race");
    let dir = root.join(todo::DIR);
    fs::create_dir_all(&dir).unwrap();
    let day = today();
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let mut errs = Vec::new();
                for i in 0..30 {
                    if let Err(e) = todo::log_checkin(&dir, day, &format!("reply {t}-{i}"), &[], &[]) {
                        errs.push(e);
                    }
                }
                errs
            })
        })
        .collect();
    let errs: Vec<String> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
    assert!(errs.is_empty(), "{} of 240 check-in logs failed, first: {:?}", errs.len(), errs.first());
    // Each write must also be in the log: a write that lost a race returns Ok and is gone.
    let log = fs::read_to_string(dir.join(todo::CHECKIN_LOG)).unwrap();
    assert_eq!(log.lines().filter(|l| l.starts_with("## ")).count(), 240, "check-in log entries lost under concurrent writes");
    for t in 0..8 {
        for i in 0..30 {
            assert!(log.lines().any(|l| l == format!("reply {t}-{i}")), "reply {t}-{i} missing from the log");
        }
    }
}

// BREAK 7: a message received days before the night DM (catch-up replay) is treated as the reply.
#[tokio::test]
async fn b7_message_from_before_the_night_dm_is_not_a_reply() {
    let _g = guard();
    let (inbox, root) = setup("stale");
    let day = today();
    let dir = write_day(&root, day, "- finish lab report\n");
    daily::stage(&inbox, Slot::Night, &ymd(day)).unwrap();
    let chat = Seq::new(&[r#"{"checkin":true,"done":[1],"tomorrow":[]}"#, "ok", "ok"]);
    let (out, replies) = say(&inbox, &root, chat, 17, "lab report draft is in my notes", Some(RECEIVED)).await;
    let body = fs::read_to_string(todo::day_file(&dir, day)).unwrap();
    assert!(!body.contains("~~"), "a message from before the night DM struck a to-do; outcome {out:?}, replies {replies:?}");
}

// BREAK 8: the model returns numbers as strings; they are dropped and nothing is struck.
#[test]
fn b8_string_item_numbers_are_honoured() {
    let a = daily::parse_answer(r#"{"checkin":true,"done":["1"],"tomorrow":[]}"#, 2).expect("parses");
    assert_eq!(a.done, vec![1], "numbers sent as strings are dropped");
}

// BREAK 9: a horizontal rule in a to-do body becomes an open item.
#[test]
fn b9_horizontal_rule_is_not_a_todo() {
    let _g = guard();
    let (_inbox, root) = setup("hr");
    let day = today();
    let dir = write_day(&root, day, "- a\n---\n- b\n");
    let texts: Vec<String> = todo::day_items(&dir, day).into_iter().map(|i| i.text).collect();
    assert_eq!(texts, vec!["a".to_string(), "b".to_string()], "a rule line is not a to-do");
}

// BREAK 10 (low): a BOM before the front matter turns the YAML into open items.
#[test]
fn b10_bom_before_front_matter() {
    let _g = guard();
    let (_inbox, root) = setup("bom");
    let day = today();
    let dir = write_day(&root, day, "\u{feff}---\ntags:\n  - todo\n---\n- write essay\n");
    let texts: Vec<String> = todo::day_items(&dir, day).into_iter().map(|i| i.text).collect();
    assert_eq!(texts, vec!["write essay".to_string()], "front matter read as to-dos");
}

// PASS CHECKS: window edges and second replies. Expected to pass; kept to show they were tried.
#[test]
fn p1_midnight_rollover_still_awaits_yesterday() {
    let _g = guard();
    let (inbox, _root) = setup("midnight");
    daily::stage(&inbox, Slot::Night, "2026-10-27").unwrap();
    let t = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap().and_hms_opt(0, 10, 0).unwrap();
    assert_eq!(daily::awaiting(&inbox, t), Some("2026-10-27".to_string()));
    let t2 = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap().and_hms_opt(0, 10, 0).unwrap();
    assert_eq!(daily::awaiting(&inbox, t2), None);
}

#[tokio::test]
async fn p2_second_reply_after_answer_files_normally() {
    let _g = guard();
    let (inbox, root) = setup("second");
    let day = today();
    write_day(&root, day, "- finish lab report\n");
    daily::stage(&inbox, Slot::Night, &ymd(day)).unwrap();
    let chat = Seq::new(&[r#"{"checkin":true,"done":[],"tomorrow":[]}"#, "ok", "ok"]);
    let _ = say(&inbox, &root, chat.clone(), 21, "nothing done", None).await;
    let (out, _) = say(&inbox, &root, chat, 22, "also here is a receipt note", None).await;
    assert!(has_note(&inbox), "second reply must file; outcome {out:?}");
}
