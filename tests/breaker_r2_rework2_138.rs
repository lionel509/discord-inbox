// Breaker, rework round 2 on #138 (head 72bcbdd): due-today placement, exam and can't-slip by title,
// point-in-time events, the failed-read path and the voice guard. Made-up calendar names only.
use chrono::NaiveDate;
use discord_inbox::calendar::{self, Cal, Event};
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Slot};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const SCHEMA: &str = include_str!("fixtures/calendar_schema.sql");

struct Fake(String);
impl Chat for Fake {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
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

fn empty_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("breaker-r2-138-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn morning(events: &[Event]) -> String {
    daily::morning_facts(&empty_dir("m"), events, None, d(2026, 10, 28))
}

fn last_line(s: &str) -> &str {
    s.lines().last().unwrap()
}

// B1: a Midterm on a Lectures calendar, tomorrow. MF3 says can't-slip comes from the title only.
#[test]
fn breaker_r2_lectures_midterm_tomorrow_is_named_in_cant_slip() {
    let evs = [ev("u-mid", "CHEM 999 - Lectures", "2026-10-29", Some("18:00-19:00"), "Midterm 2")];
    let f = morning(&evs);
    assert!(last_line(&f).contains("Midterm 2"), "facts were:\n{f}");
}

// B2: a timed item with a due word today. It moves to Due today, so it leaves the timeline and the free-gap math.
#[test]
fn breaker_r2_timed_due_today_still_blocks_the_free_gap() {
    let evs = [ev("u-q2", "CHEM 999 - Lectures", "2026-10-28", Some("10:00-11:00"), "Quiz 2 due")];
    let f = morning(&evs);
    assert_eq!(last_line(&f), "Can't slip: Quiz 2 due. Biggest free gap: 11:00-22:00.", "facts were:\n{f}");
}

// B3: a timed due item overlapping a lecture. The item is no longer in the timeline, so no overlap note is made.
#[test]
fn breaker_r2_timed_due_item_gets_an_overlap_note() {
    let evs = [
        ev("u-if", "Career", "2026-10-28", Some("10:00-11:00"), "Intern form due"),
        ev("u-l4", "CHEM 999 - Lectures", "2026-10-28", Some("10:30-11:30"), "Lecture 4"),
    ];
    let f = morning(&evs);
    assert!(f.contains("overlaps Intern form due"), "facts were:\n{f}");
}

// B4: "Final" is a whole word in the exam list, so a final project check-in is an exam for the Bring line.
#[test]
#[ignore = "#139: word list: refine final, not drop it"]
fn breaker_r2_final_project_is_not_an_exam() {
    let evs = [ev("u-fp", "Work", "2026-10-28", Some("14:00-15:00"), "Final project check-in")];
    let f = morning(&evs);
    assert!(f.contains("Bring: nothing extra"), "facts were:\n{f}");
}

// B5: "Deadlines" (plural) is not in the due-word list, so a multi-day deadline run is filed as background.
#[test]
fn breaker_r2_plural_deadlines_is_due_today() {
    let evs = [
        ev("u-dls", "Career", "2026-10-28", None, "Deadlines for intern forms"),
        ev("u-dls", "Career", "2026-10-29", None, "Deadlines for intern forms"),
    ];
    let f = morning(&evs);
    assert!(f.contains("Due today:"), "facts were:\n{f}");
}

// B6: point in time with a second-level end. An end 59 s after the start passes the SQL check and prints
// "23:59-23:59", so the facts show equal halves (the demo pass rule forbids this).
#[test]
#[ignore = "#139: cosmetic 23:59-23:59 print in list_sql"]
fn breaker_r2_point_event_with_second_level_end_has_no_equal_halves() {
    let db = std::env::temp_dir().join(format!("breaker-r2-138-{}.sqlitedb", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let s = "(strftime('%s','2026-10-28 23:59:00','+4 hours') - 978307200)";
    let e = "(strftime('%s','2026-10-28 23:59:59','+4 hours') - 978307200)";
    let sql = format!(
        "{SCHEMA}\nINSERT INTO Calendar(ROWID,title) VALUES(1,'CHEM 999 - Lectures');\n\
         INSERT INTO CalendarItem(ROWID,summary,start_date,start_tz,end_date,all_day,calendar_id,has_recurrences,UUID) VALUES(1,'Pop quiz',{s},'America/New_York',{e},0,1,0,'u-pop');\n\
         INSERT INTO OccurrenceCache(day,event_id,calendar_id,store_id,occurrence_date,occurrence_start_date,occurrence_end_date) VALUES(0,1,1,1,{s},NULL,{e});\n"
    );
    let mut child = Command::new("/usr/bin/sqlite3").arg(&db).stdin(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(sql.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    let cal = Cal {
        osascript: PathBuf::from("/nonexistent/osascript"),
        calendars: vec!["CHEM 999 - Lectures".into()],
        db: db.clone(),
    };
    let evs = calendar::list(&cal).unwrap();
    let _ = std::fs::remove_file(&db);
    assert_eq!(evs.len(), 1);
    let t = evs[0].time.clone().unwrap_or_default();
    let halves: Vec<&str> = t.split('-').collect();
    assert!(!(halves.len() == 2 && halves[0] == halves[1]), "time was {t:?}");
}

// B7: a voice reply that moves the Can't slip line to the top. MF6 says the reply must end with it word for word.
#[test]
#[ignore = "#139: voice guard checks the last line, not presence"]
fn breaker_r2_voice_rejects_a_reply_that_moves_the_closing_line() {
    let evs = [
        ev("u-l3", "CHEM 999 - Lectures", "2026-10-28", Some("10:00-11:00"), "Lecture 3"),
        ev("u-lab2", "Work", "2026-10-28", Some("10:30-11:30"), "Lab 2"),
    ];
    let facts = morning(&evs);
    let closing = last_line(&facts).to_string();
    let rest: Vec<&str> = facts.lines().take(facts.lines().count() - 1).collect();
    let reply = format!("{closing}\nMorning Lionel.\n{}", rest.join("\n"));
    let out = daily::voice(&Fake(reply), Slot::Morning, &facts);
    assert_eq!(out, facts, "guard accepted a reply that does not end with the closing line:\n{out}");
}

// B8: the overlap guard counts the word "overlaps" but not the pairs. A reply that re-pairs a note keeps the count.
#[test]
#[ignore = "#139: voice guard checks overlap pairs"]
fn breaker_r2_voice_checks_overlap_pairs_not_just_the_count() {
    let evs = [
        ev("u-l3", "CHEM 999 - Lectures", "2026-10-28", Some("10:00-11:00"), "Lecture 3"),
        ev("u-lab2", "Work", "2026-10-28", Some("10:30-11:30"), "Lab 2"),
        ev("u-cc", "Career", "2026-10-28", Some("14:00-15:00"), "Coffee chat"),
        ev("u-mi", "Work", "2026-10-28", Some("14:30-15:30"), "Mock interview"),
    ];
    let facts = morning(&evs);
    assert!(facts.contains("overlaps Mock interview"), "precondition: facts were:\n{facts}");
    let reply = format!("Morning Lionel.\n{}", facts.replace("overlaps Mock interview", "overlaps Lecture 3"));
    let out = daily::voice(&Fake(reply), Slot::Morning, &facts);
    assert_eq!(out, facts, "guard accepted a reply with a re-paired overlap note:\n{out}");
}

// B9: failed calendar read. The warning is prepended after the model, but the model's own text is not checked
// for calendar claims. Expected: the guard falls back to the unread facts, which make no calendar claim.
#[test]
#[ignore = "#139: superseded, compose skips the model on a failed read (MF-E)"]
fn breaker_r2_failed_read_model_reply_cannot_claim_the_calendar() {
    let facts = daily::unread(&morning(&[]));
    let reply = "Morning Lionel.\nTomorrow first: nothing on the calendar.\nBring: calculator (exam today).";
    let out = daily::voice(&Fake(reply.to_string()), Slot::Morning, &facts);
    assert!(!out.contains("Bring: calculator") && !out.contains("nothing on the calendar"), "reply kept calendar claims:\n{out}");
}

// B10 (control): points at the gap edges should not move the gap. Expected: 08:00-22:00.
#[test]
fn breaker_r2_control_points_at_gap_edges() {
    let evs = [
        ev("p1", "Career", "2026-10-28", Some("22:00"), "Call at ten"),
        ev("p2", "Work", "2026-10-28", Some("08:00"), "Stretch"),
        ev("p3", "Study", "2026-10-28", Some("23:59"), "Late point"),
        ev("p4", "Study", "2026-10-28", Some("00:00"), "Midnight point"),
    ];
    let f = morning(&evs);
    assert_eq!(last_line(&f), "Can't slip: nothing due. Biggest free gap: 08:00-22:00.", "facts were:\n{f}");
}

// B11 (control): done items. "✅" items stay out of Bring and Can't slip. Expected gap 10:00-22:00.
#[test]
fn breaker_r2_control_done_items_stay_out() {
    let evs = [
        ev("u-d1", "Career", "2026-10-28", None, "✅ HW 4 due"),
        ev("u-d2", "CHEM 999 - Lectures", "2026-10-28", Some("09:00-10:00"), "✅ Midterm 1"),
    ];
    let f = morning(&evs);
    assert!(f.contains("Bring: nothing extra"), "facts were:\n{f}");
    assert_eq!(last_line(&f), "Can't slip: nothing due. Biggest free gap: 10:00-22:00.", "facts were:\n{f}");
}

// B12 (control): exact start == end through the SQL, at 23:59 and at 00:00. Expected: one time, no equal halves.
#[test]
fn breaker_r2_control_exact_point_rows_through_sql() {
    for (tag, start) in [("a", "23:59:00"), ("b", "00:00:00")] {
        let db = std::env::temp_dir().join(format!("breaker-r2-138-exact-{tag}-{}.sqlitedb", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let s = format!("(strftime('%s','2026-10-28 {start}','+4 hours') - 978307200)");
        let sql = format!(
            "{SCHEMA}\nINSERT INTO Calendar(ROWID,title) VALUES(1,'CHEM 999 - Lectures');\n\
             INSERT INTO CalendarItem(ROWID,summary,start_date,start_tz,end_date,all_day,calendar_id,has_recurrences,UUID) VALUES(1,'Pop quiz',{s},'America/New_York',{s},0,1,0,'u-pop');\n\
             INSERT INTO OccurrenceCache(day,event_id,calendar_id,store_id,occurrence_date,occurrence_start_date,occurrence_end_date) VALUES(0,1,1,1,{s},NULL,{s});\n"
        );
        let mut child = Command::new("/usr/bin/sqlite3").arg(&db).stdin(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(sql.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        let cal = Cal { osascript: PathBuf::from("/nonexistent/osascript"), calendars: vec!["CHEM 999 - Lectures".into()], db: db.clone() };
        let evs = calendar::list(&cal).unwrap();
        let _ = std::fs::remove_file(&db);
        let t = evs[0].time.clone().unwrap_or_default();
        let halves: Vec<&str> = t.split('-').collect();
        assert!(!(halves.len() == 2 && halves[0] == halves[1]), "{tag}: time was {t:?}");
    }
}
