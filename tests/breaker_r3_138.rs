// Breaker round 3 (PR #138, head f1e73ec). Each test is a repro: a failing test is a break.
use chrono::NaiveDate;
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Deps, Slot};
use discord_inbox::todo;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Canned(String);
impl Chat for Canned {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("di-breaker-r3-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
}

fn deps(todo: &Path, model: &str) -> Deps {
    Deps { todo_dir: todo.to_path_buf(), cal: None, chat: Arc::new(Canned(model.to_string())) }
}

// main.rs stages the night marker, then composes (a model call), then saves the list and sends.
// A message received in that window is treated as the reply to a DM that has not gone out.
#[test]
#[ignore = "follow-up #139"]
fn r3_message_before_the_list_is_saved_is_not_a_reply() {
    let inbox = tmp("stage-window");
    let d = "2026-10-20";
    daily::stage(&inbox, Slot::Night, d).unwrap();
    assert!(daily::load_list(&inbox, d).is_none());
    let received = (chrono::Utc::now() + chrono::TimeDelta::seconds(60)).to_rfc3339();
    assert!(
        !daily::after_send(&inbox, d, &received),
        "a message received before the night list is saved counts as the reply and is read against the live list"
    );
}

// The guard is a word list, not the class "rain gear or the chance of rain" the prompt forbids.
#[test]
#[ignore = "follow-up #139"]
fn r3_voice_rain_gear_slips_past_the_guard() {
    let facts = "Weather: unavailable\nWear: no change\nBring: nothing extra\nToday:\n- 09:30 Lab check-in (CHEM 101 - Work)";
    let slips = [
        "Take a waterproof jacket, the lab check-in runs long today so plan the walk.",
        "\u{2602} Take your brolly on the way out, the lab check-in runs long today.",
        "\u{1f327} The lab check-in runs long today, so plan the walk home carefully.",
        "Drizzle is likely around the lab check-in, so dress for the walk today.",
        "Expect showers around the lab check-in, so plan the walk home today.",
    ];
    let mut through = Vec::new();
    for s in slips {
        if daily::voice(&Canned(s.to_string()), Slot::Morning, facts) == s {
            through.push(s);
        }
    }
    assert!(through.is_empty(), "rain gear or rain named in the reply, and the reply went out unchanged: {through:#?}");
}

// The DM is the model's voice of the facts. When the voice drops the numbered list, Lionel
// sees no numbers, but the saved list (and so the reply's numbers) still has them.
#[test]
fn r3_numbered_list_is_in_the_dm_the_reply_refers_to() {
    let root = tmp("dm-numbers");
    let todo = root.join(todo::DIR);
    fs::create_dir_all(&todo).unwrap();
    fs::write(todo.join("2026-10-20.md"), "- alpha\n- beta\n").unwrap();
    let voiced = "Good evening Lionel. Nothing urgent is waiting, so rest well and sleep easy tonight.";
    let dm = daily::compose(&deps(&todo, voiced), Slot::Night, day());
    assert!(
        dm.contains("1. alpha") && dm.contains("2. beta"),
        "the DM Lionel reads has no numbers, so 'done 2' points at nothing he saw. DM was: {dm:?}"
    );
}

// `is_open` now needs a letter or digit. A to-do that is only an emoji is dropped from the list and never carried.
#[test]
#[ignore = "follow-up #139"]
fn r3_emoji_only_todo_line_is_still_an_item() {
    let root = tmp("emoji-item");
    let todo = root.join(todo::DIR);
    fs::create_dir_all(&todo).unwrap();
    fs::write(todo.join("2026-10-20.md"), "- \u{1f4da}\n- alpha\n").unwrap();
    let texts: Vec<String> = todo::day_items(&todo, day()).into_iter().map(|i| i.text).collect();
    assert_eq!(texts, vec!["\u{1f4da}".to_string(), "alpha".to_string()]);
}

// apply() strikes the day file, carries the rest, then log_checkin() fails on a symlinked log.
// answer() returns Err, which the ingest hook reads as "filed as usual", but the strike is already on disk.
#[test]
#[ignore = "follow-up #139"]
fn r3_failed_log_write_after_strikes_leaves_the_day_file_as_it_was() {
    let root = tmp("log-symlink");
    let todo = root.join(todo::DIR);
    fs::create_dir_all(&todo).unwrap();
    let before = "- alpha\n- beta\n";
    fs::write(todo.join("2026-10-20.md"), before).unwrap();
    let outside = root.join("outside.md");
    fs::write(&outside, "keep\n").unwrap();
    std::os::unix::fs::symlink(&outside, todo.join(todo::CHECKIN_LOG)).unwrap();
    let model = r#"{"checkin":true,"done":[1],"tomorrow":[]}"#;
    let r = daily::answer(&deps(&todo, model), day(), "alpha is done", None);
    let after = fs::read_to_string(todo.join("2026-10-20.md")).unwrap();
    assert!(r.is_err(), "the log write should be refused on a symlink");
    assert_eq!(after, before, "answer() returned Err (read as nothing written), but the day file was already struck: {after:?}");
}

// log_checkin() flattens done and carried, but not the reply text itself.
#[test]
#[ignore = "follow-up #139"]
fn r3_newline_in_the_reply_cannot_start_a_heading_in_the_log() {
    let root = tmp("reply-newline");
    let todo = root.join(todo::DIR);
    fs::create_dir_all(&todo).unwrap();
    todo::log_checkin(&todo, day(), "done\n## 2030-01-01\nDone: everything", &["alpha".to_string()], &[]).unwrap();
    let log = fs::read_to_string(todo.join(todo::CHECKIN_LOG)).unwrap();
    let headings: Vec<&str> = log.lines().filter(|l| l.starts_with("## ")).collect();
    assert_eq!(headings, vec!["## 2026-10-20"], "the reply's newline made a second heading in the log: {log:?}");
}

// remap_done() matches by text and takes the first unused match. Two items with the same text
// (a day line and a standing note both called "Laundry") make number 2 strike the day line.
#[test]
#[ignore = "follow-up #139"]
fn r3_number_two_strikes_the_note_not_the_same_text_on_the_day_file() {
    let root = tmp("dup-remap");
    let todo = root.join(todo::DIR);
    fs::create_dir_all(&todo).unwrap();
    fs::write(todo.join("2026-10-20.md"), "- Laundry\n").unwrap();
    fs::write(todo.join("Laundry.md"), "standing note\n").unwrap();
    let sent = Some(vec!["Laundry".to_string(), "Laundry".to_string()]);
    let model = r#"{"checkin":true,"done":[2],"tomorrow":[]}"#;
    daily::answer(&deps(&todo, model), day(), "done 2", sent).unwrap();
    let note = fs::read_to_string(todo.join("Laundry.md")).unwrap();
    let day_file = fs::read_to_string(todo.join("2026-10-20.md")).unwrap();
    assert!(note.contains("done: 2026-10-20"), "number 2 (the note) was not struck. note: {note:?} day file: {day_file:?}");
}
