//! Receipt photos through `pip::handle`: the OCR stub stands in for Vision. Scratch dirs are `…/Private/Nutrition`.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAIN: u64 = 100000000000000001;
const MAIN_CH: u64 = 5001;

const SAMPLE: &str = "CAMPUS DINING - EAST\n\
Order #4821\n\
10/26/2026 22:53\n\
2PC Chicken Combo\n\
>2PC Chicken  $ 11.45\n\
Spicy\n\
>Mashed Potato W- C Ajun Gra\n\
>Soda\n\
Subtotal  $ 11.45\n\
Tax  $ 0.00\n\
Total  $ 11.45\n\
Payment Source  Meal Plan\n\
Card #  XXXXXXXXXXXX4417\n\
Remaining Balance  $ 212.30";

struct Msg {
    id: u64,
    received: &'static str,
    text: &'static str,
    files: Vec<&'static str>,
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
        MAIN_CH
    }
    fn received(&self) -> String {
        self.received.to_string()
    }
    fn content(&self) -> String {
        self.text.to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.files.iter().map(|f| f.to_string()).collect()
    }
    fn live(&self) -> bool {
        false
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(b"jpeg bytes".to_vec())
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
    async fn reply_rich(&self, r: &discord_inbox::rich::Rich) -> Result<(), String> {
        self.replies.lock().unwrap().push(format!("{:?}", r));
        Ok(())
    }
}

/// A fresh scratch nutrition dir and the OCR counter file next to it.
fn world(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pip-receipt-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Nutrition");
    fs::create_dir_all(&dir).unwrap();
    (dir, root.join("ocr-count"))
}

/// A `/bin/sh` OCR helper that counts its calls and prints `text` (or exits 1 when `text` is None).
fn stub(root: &Path, text: Option<&str>) -> PathBuf {
    let counter = root.join("ocr-count");
    let body_file = root.join("ocr-text");
    fs::write(&body_file, text.unwrap_or("")).unwrap();
    let p = root.join(format!("helper-{}.sh", text.map_or(1, |_| 0)));
    let body = match text {
        Some(_) => format!(
            "#!/bin/sh\ncat >/dev/null; echo x >> '{}'; cat '{}'\n",
            counter.display(),
            body_file.display()
        ),
        None => "#!/bin/sh\ncat >/dev/null; exit 1\n".to_string(),
    };
    fs::write(&p, body).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn count(root: &Path) -> usize {
    fs::read_to_string(root.join("ocr-count")).map(|s| s.lines().count()).unwrap_or(0)
}

fn cfg(dir: &Path, ocr: Option<PathBuf>) -> Config {
    Config { nutrition_dir: dir.to_path_buf(), recipes_dir: None, low_kcal: 1800, kcal_target: 2400, ocr }
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn msg(id: u64, received: &'static str, text: &'static str, files: Vec<&'static str>) -> (Msg, Arc<Mutex<Vec<String>>>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    (Msg { id, received, text, files, replies: replies.clone() }, replies)
}

async fn run(c: &Config, m: &Msg) -> Outcome {
    pip::handle(&allow(), c, m).await
}

fn note(dir: &Path, day: &str) -> String {
    fs::read_to_string(daylog::note_path(dir, day)).unwrap_or_default()
}

fn joined(r: &Arc<Mutex<Vec<String>>>) -> String {
    r.lock().unwrap().join("\n")
}

const SAVED_REPLY: &str = "Photo saved. I don't guess from photos; reply with what was in it (e.g. salmon 100g, cooked rice 180g).";

#[tokio::test]
async fn same_night_receipt_logs_at_the_printed_time() {
    let (dir, root) = world("same-night");
    let c = cfg(&dir, Some(stub(&root, Some(SAMPLE))));
    let (m, replies) = msg(2001, "2026-10-26T22:55:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(n.contains("### 22:53 · msg 2001"), "{n}");
    for food in ["fried chicken", "mashed potato", "gravy"] {
        assert!(n.contains(food), "{food} missing from {n}");
    }
    assert!(n.contains("soda"), "{n}");
    assert!(n.contains("receipts have no weights"), "{n}");
    let r = joined(&replies);
    assert!(r.contains("Portions are assumed"), "{r}");
    assert!(r.contains("Not counted"), "{r}");
    assert!(daylog::owed_before(&dir, "2026-10-27", u64::MAX).is_none());
    assert_eq!(count(&root), 1);
}

#[tokio::test]
async fn next_morning_receipt_goes_to_yesterday_and_replays_skip() {
    let (dir, root) = world("next-morning");
    let c = cfg(&dir, Some(stub(&root, Some(SAMPLE))));
    let (m, replies) = msg(2002, "2026-10-27T09:12:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(note(&dir, "2026-10-26").contains("### 22:53 · msg 2002"));
    assert!(!note(&dir, "2026-10-27").contains("msg 2002"));
    let r = joined(&replies);
    assert!(r.contains("yesterday's note"), "{r}");
    assert!(r.contains("#128"), "{r}");
    let before = note(&dir, "2026-10-26");
    let (again, _) = msg(2002, "2026-10-27T09:12:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&c, &again).await, Outcome::Skipped);
    assert_eq!(count(&root), 1);
    assert_eq!(note(&dir, "2026-10-26"), before);
}

#[tokio::test]
async fn printed_date_out_of_range_logs_at_send_time() {
    let (dir, root) = world("out-of-range");
    let text = "CAMPUS DINING\n10/20/2026 12:00\n>2PC Chicken  $ 11.45\nTotal  $ 11.45";
    let c = cfg(&dir, Some(stub(&root, Some(text))));
    let (m, replies) = msg(2003, "2026-10-27T09:12:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(note(&dir, "2026-10-27").contains("### 09:12 · msg 2003"));
    assert!(joined(&replies).contains("isn't today or yesterday"));
}

#[tokio::test]
async fn no_printed_date_logs_at_send_time() {
    let (dir, root) = world("no-date");
    let text = "CAMPUS DINING\n>2PC Chicken  $ 11.45\nTotal  $ 11.45";
    let c = cfg(&dir, Some(stub(&root, Some(text))));
    let (m, replies) = msg(2004, "2026-10-27T09:12:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(note(&dir, "2026-10-27").contains("### 09:12 · msg 2004"));
    assert!(joined(&replies).contains("No printed time"));
}

#[tokio::test]
async fn non_receipt_photos_are_unchanged_from_no_ocr() {
    let outputs: Vec<Option<&str>> = vec![
        Some(""),
        Some("GRILLED SALMON BOWL"),
        Some("Nutrition Facts\nCalories 250\nTotal Fat 8g 10%\nTotal Carbohydrate 30g"),
        None,
    ];
    let (dir0, _root0) = world("plain-off");
    let (m0, r0) = msg(2005, "2026-10-26T15:50:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&cfg(&dir0, None), &m0).await, Outcome::Saved);
    let want_note = note(&dir0, "2026-10-26");
    let want_reply = joined(&r0);
    assert_eq!(want_reply, SAVED_REPLY);
    for (i, out) in outputs.into_iter().enumerate() {
        let (dir, root) = world(&format!("plain-{i}"));
        let (m, r) = msg(2005, "2026-10-26T15:50:00-04:00", "", vec!["r.jpg"]);
        let c = cfg(&dir, Some(stub(&root, out)));
        assert_eq!(run(&c, &m).await, Outcome::Saved, "case {i}");
        assert_eq!(note(&dir, "2026-10-26"), want_note, "case {i}");
        assert_eq!(joined(&r), want_reply, "case {i}");
    }
}

#[tokio::test]
async fn typed_meal_and_fix_with_a_photo_never_run_ocr() {
    let (dir, root) = world("typed");
    let c = cfg(&dir, Some(stub(&root, Some(SAMPLE))));
    let (m, _) = msg(2006, "2026-10-26T12:00:00-04:00", "salmon 100g", vec!["r.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let (f, _) = msg(2007, "2026-10-26T12:05:00-04:00", "fix salmon 100g", vec!["r.jpg"]);
    let _ = run(&c, &f).await;
    assert_eq!(count(&root), 0);
}

#[tokio::test]
async fn receipt_with_nothing_readable_says_so() {
    let (dir, root) = world("nothing");
    let c = cfg(&dir, Some(stub(&root, Some("CAMPUS DINING\nTotal  $ 0.00"))));
    let (m, replies) = msg(2008, "2026-10-26T12:00:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(joined(&replies).starts_with("That looks like a receipt"));
}

#[tokio::test]
async fn receipt_text_never_reaches_the_note_or_reply() {
    let (dir, root) = world("privacy");
    let c = cfg(&dir, Some(stub(&root, Some(SAMPLE))));
    let (m, replies) = msg(2009, "2026-10-26T22:55:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let all = format!("{}\n{}", note(&dir, "2026-10-26"), joined(&replies));
    for secret in ["4417", "212.30", "4821", "Balance", "Card"] {
        assert!(!all.contains(secret), "{secret} leaked: {all}");
    }
}

#[tokio::test]
async fn failed_day_note_write_stays_owed() {
    let (dir, root) = world("write-fail");
    fs::create_dir_all(daylog::note_path(&dir, "2026-10-26")).unwrap();
    let c = cfg(&dir, Some(stub(&root, Some(SAMPLE))));
    let (m, _) = msg(2010, "2026-10-27T09:12:00-04:00", "", vec!["r.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Failed);
    assert_eq!(daylog::owed_before(&dir, "2026-10-27", u64::MAX).map(|(id, _)| id), Some(2010));
}
