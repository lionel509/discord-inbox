// Breaker on the #135 rework (head ad7f928): the debrief facts, the voice guard and the visible calendar failure.
// Made-up calendar names only. Every database is a temp file built here; the live store is never opened.
use chrono::NaiveDate;
use discord_inbox::calendar::{Cal, Event};
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Deps, Slot};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

const SCHEMA: &str = include_str!("fixtures/calendar_schema.sql");

struct Fake(String);
impl Chat for Fake {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

struct Down;
impl Chat for Down {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Err("model down".into())
    }
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("breaker-138-rework-{name}-{}", std::process::id()))
}

fn empty_dir(name: &str) -> PathBuf {
    let p = tmp(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn build_db(name: &str, sql: &str) -> PathBuf {
    let path = tmp(&format!("{name}.sqlitedb"));
    let _ = std::fs::remove_file(&path);
    let mut child = Command::new("/usr/bin/sqlite3").arg(&path).stdin(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(sql.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    path
}

fn cal_db(names: &[&str], db: PathBuf) -> Cal {
    Cal {
        osascript: PathBuf::from("/nonexistent/osascript"),
        calendars: names.iter().map(|s| s.to_string()).collect(),
        db,
    }
}

fn ev(uid: &str, calendar: &str, date: &str, time: Option<&str>, summary: &str) -> Event {
    Event {
        uid: uid.into(),
        calendar: calendar.into(),
        date: date.into(),
        time: time.map(str::to_string),
        summary: summary.into(),
    }
}

// Calendar.app's DM says "nothing on the calendar" only when the read worked. When the read failed, the DM must not say it.
#[test]
fn breaker_night_dm_does_not_claim_nothing_on_the_calendar_when_the_read_failed() {
    let deps = Deps {
        todo_dir: empty_dir("b1"),
        cal: Some(cal_db(&["CHEM 103 - Lectures"], PathBuf::from("/nonexistent/Calendar.sqlitedb"))),
        chat: Arc::new(Down),
    };
    let out = daily::compose(&deps, Slot::Night, d(2026, 10, 28));
    assert!(out.starts_with(daily::CAL_FAIL), "warning is not first: {out:?}");
    assert!(!out.contains("nothing on the calendar"), "a failed read still claims the calendar is empty: {out:?}");
}

// A zero-length event (start == end, as 155 cache rows in the real store are) is a point in time. The gap rule makes "end at or before start" a day-long span.
#[test]
fn breaker_point_event_does_not_overlap_an_afternoon_event() {
    let evs = vec![
        ev("u-checkin", "CHEM 103 - Lectures", "2026-10-28", Some("10:00-10:00"), "Lab check-in"),
        ev("u-lab", "CHEM 103 - Exams", "2026-10-28", Some("14:00-15:00"), "Lab practical"),
    ];
    let mf = daily::morning_facts(&empty_dir("b3a"), &evs, None, d(2026, 10, 28));
    let lab = mf.lines().find(|l| l.starts_with("- 14:00-15:00 ")).expect("14:00 line present");
    assert!(!lab.contains("overlaps"), "a 10:00 point event is not busy at 14:00: {lab}");
}

#[test]
fn breaker_point_event_blocks_no_time_in_the_biggest_gap() {
    let evs = vec![ev("u-checkin", "CHEM 103 - Lectures", "2026-10-28", Some("10:00-10:00"), "Lab check-in")];
    let mf = daily::morning_facts(&empty_dir("b3b"), &evs, None, d(2026, 10, 28));
    assert_eq!(mf.lines().last(), Some("Can't slip: nothing due. Biggest free gap: 08:00-22:00."), "{mf}");
}

// Study blocks are not deadlines (addendum A3). An all-day study block today is listed under Due today and named in Can't slip.
#[test]
fn breaker_all_day_study_block_is_not_named_as_cant_slip() {
    let evs = vec![ev("u-study-day", "Study", "2026-10-28", None, "Study — Block day")];
    let mf = daily::morning_facts(&empty_dir("b4"), &evs, None, d(2026, 10, 28));
    let last = mf.lines().last().unwrap_or("");
    assert!(!last.contains("Study"), "a study block is named as cannot-slip: {last}");
}

// A calendar that lists titles but has no cached occurrences is a read that returned nothing. It must not go out as an empty calendar with no warning.
#[test]
#[ignore = "#139: empty occurrence cache"]
fn breaker_empty_occurrence_cache_is_not_a_silent_empty_calendar() {
    let sql = format!(
        "{SCHEMA}\nINSERT INTO Calendar (ROWID, title) VALUES (1, 'CHEM 103 - Lectures');\nINSERT INTO CalendarItem (ROWID, summary, start_date, start_tz, all_day, calendar_id, has_recurrences, UUID) VALUES (1, 'CHEM 103 Lecture', 814802400, 'America/New_York', 0, 1, 0, 'u-x');\n"
    );
    let cal = cal_db(&["CHEM 103 - Lectures"], build_db("b5", &sql));
    let (evs, warn) = daily::read_events(Some(&cal));
    assert!(evs.is_empty());
    assert!(warn.is_some(), "an empty read with no warning goes out as 'nothing on the calendar'");
}

// Addendum A6 and the plan's "word for word" promise: the voice guard only checks numbered lines, so a reply that drops the overlap note and the closing line is sent as is.
#[test]
fn breaker_voice_does_not_send_a_reply_without_the_closing_line_or_overlap_note() {
    let evs = vec![
        ev("u-a", "CHEM 103 - Lectures", "2026-10-28", Some("10:00-11:00"), "Lecture A"),
        ev("u-b", "CHEM 103 - Exams", "2026-10-28", Some("10:30-12:00"), "Lab B"),
    ];
    let facts = daily::morning_facts(&empty_dir("b6"), &evs, None, d(2026, 10, 28));
    assert!(facts.contains("overlaps"), "setup: {facts}");
    let kept: Vec<String> = facts
        .lines()
        .filter(|l| !l.starts_with("Can't slip"))
        .map(|l| l.replace(", overlaps Lab B", "").replace(", overlaps Lecture A", ""))
        .collect();
    let reply = format!("{}\nGood morning Lionel, here is today in order.", kept.join("\n"));
    let got = daily::voice(&Fake(reply.clone()), Slot::Morning, &facts);
    assert_eq!(got, facts, "the reply dropped the closing line and the overlap note but was sent: {got:?}");
}
