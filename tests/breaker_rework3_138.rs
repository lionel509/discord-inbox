// Breaker round 3 on PR #138 (head 9b1181a), plus the reviewer's two live tests for the Can't slip fix.
// Made-up calendar names only. Breaks 1 and 2 and the two review tests are live. Breaks 3, 4 and 5 are follow-up #139.
use chrono::NaiveDate;
use discord_inbox::calendar::Event;
use discord_inbox::daily;
use std::path::PathBuf;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn empty_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("breaker-138-rework3-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
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

fn morning(evs: &[Event]) -> String {
    daily::morning_facts(&empty_dir("m"), evs, None, d(2026, 10, 28))
}

fn last_line(s: &str) -> &str {
    s.lines().last().unwrap()
}

// Break 1: an exam that is also Due today uses up a fill slot, so Due soon is dropped.
#[test]
fn r3_1_exam_also_due_today_leaves_room_for_due_soon() {
    let evs = [
        ev("u-q3", "ESE 999 - Exams", "2026-10-28", None, "Quiz 3"),
        ev("u-es", "Career", "2026-10-30", None, "Essay due Fri"),
    ];
    let f = morning(&evs);
    assert_eq!(last_line(&f), "Can't slip: Quiz 3, Essay due Fri. Biggest free gap: 08:00-22:00.", "facts were:\n{f}");
}

// Break 2: two different exams with the same title on two calendars are named once.
#[test]
fn r3_2_same_title_exams_on_two_calendars_are_both_named() {
    let evs = [
        ev("u-m1", "BIO 101 - Exams", "2026-10-29", None, "Midterm"),
        ev("u-m2", "CHEM 103 - Exams", "2026-10-30", None, "Midterm"),
    ];
    let f = morning(&evs);
    assert_eq!(last_line(&f).matches("Midterm").count(), 2, "facts were:\n{f}");
}

// Review (round 3, finding 1, second trigger): an exam tomorrow takes the fill slot again, and the essay due the day after is dropped.
#[test]
fn r3_review_exam_tomorrow_with_essay_due_the_day_after_names_both() {
    let evs = [
        ev("u-mid", "ESE 999 - Exams", "2026-10-29", None, "Midterm"),
        ev("u-es", "Career", "2026-10-30", None, "Essay due Fri"),
    ];
    let f = morning(&evs);
    assert_eq!(last_line(&f), "Can't slip: Midterm, Essay due Fri. Biggest free gap: 08:00-22:00.", "facts were:\n{f}");
}

// Review (round 3, finding 2 span rule): a two-day all-day exam span is named once, not once per day.
#[test]
fn r3_review_two_day_exam_span_is_named_once() {
    let evs = [
        ev("u-span", "ESE 999 - Exams", "2026-10-29", None, "Final exam"),
        ev("u-span", "ESE 999 - Exams", "2026-10-30", None, "Final exam"),
    ];
    let f = morning(&evs);
    assert_eq!(last_line(&f), "Can't slip: Final exam. Biggest free gap: 08:00-22:00.", "facts were:\n{f}");
}

// Break 3: two timed due items that overlap each other get no note. Follow-up #139.
#[test]
#[ignore = "follow-up: #139"]
fn r3_5_two_timed_due_items_that_overlap_each_other_are_flagged() {
    let evs = [
        ev("u-q2", "CHEM 999 - Lectures", "2026-10-28", Some("10:00-11:00"), "Quiz 2 due"),
        ev("u-lab", "CHEM 999 - Lectures", "2026-10-28", Some("10:30-11:30"), "Lab 4 due"),
    ];
    let f = morning(&evs);
    assert!(f.contains("overlaps Lab 4 due") || f.contains("overlaps Quiz 2 due"), "no overlap note for two due items:\n{f}");
}

// Break 4: "nothing due" in a lecture title is read as a due word. Follow-up #139.
#[test]
#[ignore = "follow-up: #139"]
fn r3_7_not_due_in_a_lecture_title_is_not_a_deadline() {
    let evs = [ev("u-rd", "CHEM 999 - Lectures", "2026-10-29", None, "Reading, nothing due this week")];
    let f = morning(&evs);
    assert!(!f.contains("Due soon"), "lecture with 'nothing due' listed as due soon:\n{f}");
}

// Break 5: "review session" and "prep" are named as exams. Follow-up #139.
#[test]
#[ignore = "follow-up: #139"]
fn r3_8_exam_review_session_is_not_named_as_an_exam() {
    let evs = [
        ev("u-rs", "CHEM 999 - Lectures", "2026-10-29", Some("18:00-19:00"), "Exam review session"),
        ev("u-ep", "Study", "2026-10-30", None, "exam prep"),
    ];
    let f = morning(&evs);
    assert_eq!(last_line(&f), "Can't slip: nothing due. Biggest free gap: 08:00-22:00.", "facts were:\n{f}");
}
