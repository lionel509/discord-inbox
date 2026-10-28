// Calendar reads from a scratch copy of Calendar's database (#135 rework): the query, the privacy filter, the failure paths, and the DM facts built from it. The live database is never opened; every Cal here points at the fixture, a temp file, or /nonexistent.
use chrono::NaiveDate;
use discord_inbox::calendar::{self, Cal, Event, Finish};
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Deps, Slot};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const SCHEMA: &str = include_str!("fixtures/calendar_schema.sql");
const ROWS: &str = include_str!("fixtures/calendar_rows.sql");
const CHEM: &[&str] = &["CHEM 103 - Lectures", "CHEM 103 - Exams"];
const ALL: &[&str] = &["CHEM 103 - Lectures", "CHEM 103 - Exams", "Career", "Study"];

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calendar.sqlitedb")
}

fn cal(names: &[&str]) -> Cal {
    Cal {
        osascript: PathBuf::from("/nonexistent/osascript"),
        calendars: names.iter().map(|s| s.to_string()).collect(),
        db: fixture(),
    }
}

fn cal_db(names: &[&str], db: PathBuf) -> Cal {
    Cal { db, ..cal(names) }
}

// Builds a database file from SQL on sqlite3's stdin.
fn build_db(path: &Path, sql: &str) {
    let _ = std::fs::remove_file(path);
    let mut child = Command::new("/usr/bin/sqlite3").arg(path).stdin(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(sql.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
}

fn tmp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("di-cal-sqlite-{name}-{}", std::process::id()))
}

// An empty directory: the to-do folder for the facts tests.
fn empty_dir(name: &str) -> PathBuf {
    let p = tmp_file(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

// Scratch copies this process left in the temp dir.
fn leftovers() -> Vec<String> {
    let prefix = format!("discord-inbox-cal-{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(&prefix))
        .collect()
}

type Triple = (String, String, Option<String>);

fn triples(evs: &[Event]) -> Vec<Triple> {
    evs.iter().map(|e| (e.uid.clone(), e.date.clone(), e.time.clone())).collect()
}

fn t(uid: &str, date: &str, time: Option<&str>) -> Triple {
    (uid.to_string(), date.to_string(), time.map(str::to_string))
}

/// A chat that always answers the same text.
struct Fixed(String);
impl Chat for Fixed {
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

#[test]
fn list_reads_course_events_in_order() {
    let _g = lock();
    let evs = calendar::list(&cal(CHEM)).unwrap();
    assert_eq!(
        triples(&evs),
        vec![
            t("u-chem-lec", "2026-10-27", Some("10:00")),
            t("u-chem-lec", "2026-10-28", Some("10:00")),
            t("u-chem-q3", "2026-10-28", Some("23:30")),
            t("u-chem-lab5", "2026-10-29", None),
            t("u-chem-labweek", "2026-10-29", None),
            t("u-chem-lec", "2026-10-29", Some("10:00")),
            t("u-chem-labweek", "2026-10-30", None),
            t("u-chem-mid", "2026-10-30", Some("18:00")),
        ]
    );
    assert_eq!(evs[2].summary, "Quiz 3 closes");
    assert_eq!(evs[7].summary, "CHEM 103 Midterm");
}

#[test]
fn recurring_lecture_once_today_and_once_tomorrow() {
    let _g = lock();
    let evs = calendar::list(&cal(CHEM)).unwrap();
    let on = |day: &str| evs.iter().filter(|e| e.uid == "u-chem-lec" && e.date == day).count();
    assert_eq!(on("2026-10-28"), 1);
    assert_eq!(on("2026-10-29"), 1);
}

#[test]
fn all_day_keeps_its_own_date() {
    let _g = lock();
    let evs = calendar::list(&cal(CHEM)).unwrap();
    let lab5 = evs.iter().find(|e| e.summary == "Lab 5 report due").expect("lab 5 listed");
    assert_eq!(lab5.date, "2026-10-29");
    assert_eq!(lab5.time, None);
    assert!(!evs.iter().any(|e| e.date == "2026-10-28" && e.summary == "Lab 5 report due"));
}

#[test]
fn late_evening_stays_on_its_day() {
    let _g = lock();
    let evs = calendar::list(&cal(CHEM)).unwrap();
    let q3 = evs.iter().find(|e| e.summary == "Quiz 3 closes").expect("quiz listed");
    assert_eq!(q3.date, "2026-10-28");
    assert_eq!(q3.time.as_deref(), Some("23:30"));
}

#[test]
fn private_calendars_are_never_read() {
    let _g = lock();
    let evs = calendar::list(&cal(&["CHEM 103 - Exams", "Home", "Work", "important", "Lionel"])).unwrap();
    assert!(!evs.is_empty());
    assert!(evs.iter().all(|e| e.calendar == "CHEM 103 - Exams"), "{evs:?}");
    assert!(calendar::list(&cal(&["Home", "Work"])).is_err());
}

#[test]
fn privacy_invariant_holds_for_any_calendars_file() {
    let _g = lock();
    let pool = [
        "MATH 101 - Work", "PHYS 102 - Exams", "CHEM 103 - Lectures", "CHEM 103 - Exams", "CHEM 103 - Prof's Hours",
        "Home", "Work", "home", "WORK", "Important", "lionel", "Lionel", "Nope - Work",
    ];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |n: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    for i in 0..300 {
        let picked: Vec<&str> = pool.iter().copied().filter(|_| next(2) == 1).collect();
        // The real writer parses the file, so the subset goes through parse_calendars first.
        let names = calendar::parse_calendars(&picked.join("\n"));
        let c = Cal { osascript: PathBuf::from("/nonexistent/osascript"), calendars: names.clone(), db: fixture() };
        let key = ["MATH 101 - Work", "PHYS 102 - Exams", "CHEM 103 - Lectures", "CHEM 103 - Exams"];
        let has_key = names.iter().any(|n| key.contains(&n.as_str()));
        match calendar::list(&c) {
            Ok(evs) => {
                for e in &evs {
                    assert!(names.contains(&e.calendar), "iteration {i}: {e:?} is not configured");
                    assert!(!["home", "work", "important", "lionel"].contains(&e.calendar.trim().to_lowercase().as_str()), "iteration {i}: {e:?}");
                    assert!(e.uid != "u-home-1" && e.uid != "u-work-1", "iteration {i}: {e:?}");
                }
                if has_key {
                    assert!(!evs.is_empty(), "iteration {i}: {names:?} should list events");
                }
            }
            Err(_) => assert!(!has_key, "iteration {i}: {names:?} should not fail"),
        }
    }
}

#[test]
fn quote_in_calendar_name_is_escaped() {
    let _g = lock();
    assert_eq!(calendar::list(&cal(&["CHEM 103 - Prof's Hours"])), Ok(vec![]));
}

#[test]
fn missing_db_is_an_error() {
    let _g = lock();
    let c = cal_db(CHEM, PathBuf::from("/nonexistent/Calendar.sqlitedb"));
    assert!(calendar::list(&c).is_err());
}

#[test]
fn corrupt_db_is_an_error() {
    let _g = lock();
    let p = tmp_file("corrupt.sqlitedb");
    std::fs::write(&p, "not a database").unwrap();
    let out = calendar::list(&cal_db(CHEM, p.clone()));
    let _ = std::fs::remove_file(&p);
    assert!(out.is_err());
    assert!(leftovers().is_empty(), "{:?}", leftovers());
}

#[test]
fn no_configured_calendar_exists_is_an_error() {
    let _g = lock();
    assert!(calendar::list(&cal(&["Nope - Work"])).is_err());
}

#[test]
fn reads_a_copy_and_cleans_up() {
    let _g = lock();
    let before = std::fs::read(fixture()).unwrap();
    calendar::list(&cal(CHEM)).unwrap();
    assert_eq!(std::fs::read(fixture()).unwrap(), before, "the fixture changed");
    assert!(leftovers().is_empty(), "{:?}", leftovers());
}

#[test]
fn fixture_db_matches_sql() {
    let _g = lock();
    let p = tmp_file("rebuilt.sqlitedb");
    build_db(&p, &format!("{SCHEMA}{ROWS}"));
    let names = ["CHEM 103 - Lectures", "CHEM 103 - Exams", "Career", "Study"];
    let from_copy = calendar::list(&cal_db(&names, p.clone()));
    let from_fixture = calendar::list(&cal(&names));
    let _ = std::fs::remove_file(&p);
    assert_eq!(from_copy, from_fixture);
    assert!(from_fixture.is_ok());
}

#[test]
fn finish_never_offers_a_recurring_series() {
    let _g = lock();
    let out = calendar::finish(&Fixed("#1".into()), &cal(&["CHEM 103 - Lectures"]), "chem lecture", "2026-10-28");
    assert_eq!(out, Ok(Finish::NoMatch));
}

#[test]
fn morning_shows_course_events_and_due_soon() {
    let _g = lock();
    let events = calendar::list(&cal(CHEM)).unwrap();
    let dir = empty_dir("morning-chem");
    let mf = daily::morning_facts(&dir, &events, None, d(2026, 10, 28));
    assert!(mf.contains("- 10:00 CHEM 103 Lecture (CHEM 103 - Lectures)"), "{mf}");
    assert!(mf.contains("- 23:30 Quiz 3 closes (CHEM 103 - Exams)"), "{mf}");
    assert!(mf.contains("- Lab 5 report due (CHEM 103 - Exams), 2026-10-29"), "{mf}");
    assert!(mf.contains("- CHEM 103 Midterm (CHEM 103 - Exams), 2026-10-30"), "{mf}");
}

#[test]
fn due_soon_lists_a_span_once() {
    let _g = lock();
    let events = calendar::list(&cal(CHEM)).unwrap();
    let dir = empty_dir("span-chem");
    let mf = daily::morning_facts(&dir, &events, None, d(2026, 10, 28));
    assert_eq!(mf.matches("Lab week (CHEM 103 - Exams)").count(), 1, "{mf}");
}

#[test]
fn night_shows_tomorrow() {
    let _g = lock();
    let events = calendar::list(&cal(CHEM)).unwrap();
    let dir = empty_dir("night-chem");
    let nf = daily::night_facts(&dir, &events, d(2026, 10, 28));
    assert!(nf.contains("CHEM 103 Lecture"), "{nf}");
    assert!(!nf.contains("nothing on the calendar"), "{nf}");
}

#[test]
fn read_events_reports_failure() {
    let _g = lock();
    assert_eq!(daily::read_events(None), (Vec::new(), None));
    let bad = cal_db(CHEM, PathBuf::from("/nonexistent/Calendar.sqlitedb"));
    assert_eq!(daily::read_events(Some(&bad)), (Vec::new(), Some(daily::CAL_FAIL)));
    let (evs, warn) = daily::read_events(Some(&cal(CHEM)));
    assert!(!evs.is_empty());
    assert_eq!(warn, None);
}

#[test]
fn compose_puts_calendar_failure_first() {
    let _g = lock();
    let deps = Deps {
        todo_dir: empty_dir("compose-fail"),
        cal: Some(cal_db(CHEM, PathBuf::from("/nonexistent/Calendar.sqlitedb"))),
        chat: Arc::new(Down),
    };
    let text = daily::compose(&deps, Slot::Night, d(2026, 10, 28));
    assert!(text.starts_with(daily::CAL_FAIL), "{text}");
}

// The addendum's cases: the full ALL set, with Career and Study.
fn all_events() -> Vec<Event> {
    calendar::list(&cal(ALL)).unwrap()
}

fn morning_all() -> String {
    let dir = empty_dir("morning-all");
    daily::morning_facts(&dir, &all_events(), None, d(2026, 10, 28))
}

#[test]
fn crossing_midnight_is_listed_once() {
    let _g = lock();
    let evs = calendar::list(&cal(&["Study"])).unwrap();
    assert_eq!(
        triples(&evs),
        vec![t("u-study-1", "2026-10-28", Some("22:00-01:30"))],
    );
}

#[test]
fn timed_events_carry_their_end() {
    let _g = lock();
    let evs = all_events();
    let gh = evs.iter().find(|e| e.summary == "GitHub Insider").expect("insider listed");
    assert_eq!(gh.time.as_deref(), Some("11:00-13:00"));
}

#[test]
fn due_today_comes_first_with_done_state() {
    let _g = lock();
    let mut evs = all_events();
    evs.push(Event {
        uid: "u-done".into(),
        calendar: "CHEM 103 - Exams".into(),
        date: "2026-10-28".into(),
        time: Some("21:00".into()),
        summary: "✅ HW 6 due".into(),
    });
    let dir = empty_dir("due-today");
    let mf = daily::morning_facts(&dir, &evs, None, d(2026, 10, 28));
    let lines: Vec<&str> = mf.lines().collect();
    assert_eq!(lines[2], "Due today:", "{mf}");
    assert!(mf.contains("- 23:30 Quiz 3 closes (CHEM 103 - Exams)"), "{mf}");
    assert!(mf.contains("- 21:00 ✅ HW 6 due (CHEM 103 - Exams) (done)"), "{mf}");
    let pos = |s: &str| mf.find(s).unwrap_or_else(|| panic!("missing {s:?} in {mf}"));
    assert!(pos("Due today:") < pos("Timeline:"));
    assert!(pos("Timeline:") < pos("Running in the background:"));
    assert!(pos("Running in the background:") < pos("Due soon:"));
}

#[test]
fn timeline_flags_overlaps() {
    let _g = lock();
    let mf = morning_all();
    assert!(mf.contains("- 11:00-13:00 GitHub Insider (Career), overlaps Resume review"), "{mf}");
    assert!(mf.contains("- 12:30-13:30 Resume review (Career), overlaps GitHub Insider"), "{mf}");
    assert!(mf.contains("- 22:00-01:30 Study — HSC · CHEM 103 chapter 5 (Study)"), "{mf}");
    let lecture = mf.lines().find(|l| l.contains("CHEM 103 Lecture (CHEM 103 - Lectures)")).expect("lecture line");
    assert!(!lecture.contains("overlaps"), "{lecture}");
}

#[test]
fn span_is_one_background_line() {
    let _g = lock();
    let mf = morning_all();
    assert!(mf.contains("Running in the background: Hackathon week (Career) through 2026-10-29"), "{mf}");
    assert_eq!(mf.matches("Hackathon week").count(), 1, "{mf}");
}

#[test]
fn closing_names_what_cant_slip_and_the_gap() {
    let _g = lock();
    let mf = morning_all();
    assert_eq!(mf.lines().last(), Some("Can't slip: Quiz 3 closes, Lab 5 report due. Biggest free gap: 13:30-22:00."), "{mf}");
}

#[test]
fn study_blocks_and_career_meetings_are_not_deadlines() {
    let _g = lock();
    let dir = empty_dir("night-all");
    let nf = daily::night_facts(&dir, &all_events(), d(2026, 10, 27));
    let tail = nf.split("Due soon:").nth(1).unwrap_or("");
    assert!(!tail.contains("Study —") && !tail.contains("GitHub Insider"), "{nf}");
}

#[test]
fn empty_day_closing() {
    let _g = lock();
    let dir = empty_dir("empty-day");
    let mf = daily::morning_facts(&dir, &[], None, d(2026, 10, 28));
    assert!(mf.ends_with("Can't slip: nothing due. Biggest free gap: 08:00-22:00."), "{mf}");
    assert!(mf.contains("Today: nothing on the list"), "{mf}");
}

// Made-up names only. The title decides what an all-day item is, not the calendar name alone (#138 MF2).
fn all_day(uid: &str, calendar: &str, date: &str, summary: &str) -> Event {
    Event { uid: uid.into(), calendar: calendar.into(), date: date.into(), time: None, summary: summary.into() }
}

#[test]
fn deadline_span_is_due_today_not_background() {
    let _g = lock();
    let evs = vec![
        all_day("u-dl", "Career", "2026-10-28", "DEADLINE: Intern form closes tonight"),
        all_day("u-dl", "Career", "2026-10-29", "DEADLINE: Intern form closes tonight"),
    ];
    let facts = daily::morning_facts(&empty_dir("mf2a"), &evs, None, d(2026, 10, 28));
    assert!(facts.contains("Due today:\n- all day: DEADLINE: Intern form closes tonight (Career)"), "{facts}");
    assert!(!facts.contains("Running in the background"), "{facts}");
}

#[test]
fn study_day_is_background_not_due() {
    let _g = lock();
    let evs = vec![all_day("u-study-day", "Study", "2026-10-28", "Study — Block day")];
    let facts = daily::morning_facts(&empty_dir("mf2b"), &evs, None, d(2026, 10, 28));
    assert!(!facts.contains("Due today:"), "{facts}");
    assert!(facts.contains("Running in the background: Study — Block day (Study)"), "{facts}");
    assert!(!facts.contains(" through "), "{facts}");
}

// The exam check reads the title, never the calendar name (#138 MF3).
#[test]
fn bring_ignores_the_calendar_name() {
    let _g = lock();
    let evs = vec![Event {
        uid: "u-proj".into(),
        calendar: "ESE 999 - Exams".into(),
        date: "2026-10-28".into(),
        time: Some("09:00-12:00".into()),
        summary: "Matlab Lab 2 (project)".into(),
    }];
    let facts = daily::morning_facts(&empty_dir("mf3a"), &evs, None, d(2026, 10, 28));
    assert!(facts.contains("Bring: nothing extra"), "{facts}");
}

// An untitled side item is not a Can't slip item, even when its calendar is due soon (#138 MF3).
#[test]
fn cant_slip_skips_untitled_items() {
    let _g = lock();
    let evs = vec![
        all_day("u-side", "Career", "2026-10-29", "Side challenge — Wave 3"),
        all_day("u-hw7", "CHEM 103 - Exams", "2026-10-30", "HW 7 due"),
    ];
    let facts = daily::morning_facts(&empty_dir("mf3b"), &evs, None, d(2026, 10, 28));
    assert_eq!(facts.lines().last(), Some("Can't slip: HW 7 due. Biggest free gap: 08:00-22:00."), "{facts}");
}

// A timed item with no end is one instant, not a span running to the next day (#138 MF4). Its time shows as one clock time.
#[test]
fn zero_length_event_shows_one_time() {
    let _g = lock();
    let db = tmp_file("mf4-zero.sqlitedb");
    let sql = format!(
        "{SCHEMA}\nINSERT INTO Calendar (ROWID, title) VALUES (1, 'CHEM 103 - Exams');\nINSERT INTO CalendarItem (ROWID, summary, start_date, start_tz, all_day, calendar_id, has_recurrences, UUID) VALUES (1, 'Quiz 3 closes', 814937400, 'America/New_York', 0, 1, 0, 'u-zero');\nINSERT INTO OccurrenceCache (day, event_id, calendar_id, store_id, occurrence_date, occurrence_start_date, occurrence_end_date) VALUES (814852800, 1, 1, 1, 814937400, NULL, 814937400);\n"
    );
    build_db(&db, &sql);
    let evs = calendar::list(&cal_db(&["CHEM 103 - Exams"], db.clone())).unwrap();
    let _ = std::fs::remove_file(&db);
    assert_eq!(evs.len(), 1, "{evs:?}");
    assert_eq!(evs[0].time, Some("23:30".into()));
}

// A failed read makes no calendar claims in the facts (#138 MF5).
#[test]
fn unread_morning_makes_no_calendar_claims() {
    let _g = lock();
    let facts = daily::unread(&daily::morning_facts(&empty_dir("mf5"), &[], None, d(2026, 10, 28)));
    assert!(facts.starts_with("Weather"), "{facts}");
    for bad in ["Can't slip", "Bring:", "nothing on the list", "08:00-22:00"] {
        assert!(!facts.contains(bad), "{bad} in {facts}");
    }
}

// A reply that keeps the closing line and every overlap note is sent as written (#138 MF6).
#[test]
fn voice_keeps_a_reply_that_keeps_closing_and_overlaps() {
    let _g = lock();
    let evs = vec![
        Event { uid: "u-a".into(), calendar: "CHEM 103 - Lectures".into(), date: "2026-10-28".into(), time: Some("10:00-11:00".into()), summary: "Lecture A".into() },
        Event { uid: "u-b".into(), calendar: "CHEM 103 - Exams".into(), date: "2026-10-28".into(), time: Some("10:30-12:00".into()), summary: "Lab B".into() },
    ];
    let facts = daily::morning_facts(&empty_dir("mf6"), &evs, None, d(2026, 10, 28));
    assert!(facts.contains("overlaps"), "setup: {facts}");
    let reply = format!("Morning Lionel.\n{facts}");
    assert_eq!(daily::voice(&Fixed(reply.clone()), Slot::Morning, &facts), reply);
}

// A timed series is listed once in Due soon, at its nearest date (#138 MF8a).
#[test]
fn due_soon_lists_a_timed_series_once() {
    let _g = lock();
    let mk = |date: &str| Event {
        uid: "u-rq".into(),
        calendar: "CHEM 103 - Exams".into(),
        date: date.into(),
        time: Some("23:59".into()),
        summary: "Reading quiz closes".into(),
    };
    let evs = vec![mk("2026-10-29"), mk("2026-10-30")];
    let facts = daily::night_facts(&empty_dir("mf8a"), &evs, d(2026, 10, 27));
    assert_eq!(facts.matches("Reading quiz closes").count(), 1, "{facts}");
}

// The calendar read pins its own zone: the process zone does not move a timed event (#138 MF8b).
#[test]
fn tz_pin_ignores_the_process_zone() {
    let _g = lock();
    let saved = std::env::var_os("TZ");
    std::env::set_var("TZ", "UTC");
    let got = calendar::list(&cal(&["CHEM 103 - Exams"]));
    match &saved {
        Some(v) => std::env::set_var("TZ", v),
        None => std::env::remove_var("TZ"),
    }
    let evs = got.unwrap();
    let q = evs.iter().find(|e| e.summary == "Quiz 3 closes").expect("Quiz 3 closes present");
    assert_eq!(q.date, "2026-10-28");
    assert_eq!(q.time, Some("23:30".into()));
}

// Can't slip lists every unchecked item due today before any Due soon item (#138 MF-A).
#[test]
fn cant_slip_puts_unchecked_due_today_before_due_soon() {
    let _g = lock();
    let evs = vec![
        all_day("u-lab", "ESE 999 - Exams", "2026-10-28", "Lab 2 (project)"),
        all_day("u-fund", "Career", "2026-10-30", "DEADLINE: Fund application"),
    ];
    let facts = daily::morning_facts(&empty_dir("mfa1"), &evs, None, d(2026, 10, 28));
    assert_eq!(facts.lines().last(), Some("Can't slip: Lab 2 (project), DEADLINE: Fund application. Biggest free gap: 08:00-22:00."), "{facts}");
}

// Every unchecked due-today item is named, in its order, with no cap (#138 MF-A).
#[test]
fn cant_slip_names_every_unchecked_due_today_item() {
    let _g = lock();
    let evs = vec![
        all_day("u-a", "ESE 999 - Exams", "2026-10-28", "HW 7 due"),
        all_day("u-b", "ESE 999 - Exams", "2026-10-28", "Lab 3 report"),
        all_day("u-c", "ESE 999 - Exams", "2026-10-28", "Problem set 4"),
        all_day("u-d", "ESE 999 - Exams", "2026-10-28", "✅ HW 6 due"),
    ];
    let facts = daily::morning_facts(&empty_dir("mfa2"), &evs, None, d(2026, 10, 28));
    assert_eq!(facts.lines().last(), Some("Can't slip: HW 7 due, Lab 3 report, Problem set 4. Biggest free gap: 08:00-22:00."), "{facts}");
}

// A due-titled item on a Lectures calendar is in Due soon and in Can't slip (#138 MF-C).
#[test]
fn due_soon_lists_a_due_titled_lectures_item() {
    let _g = lock();
    let evs = vec![Event {
        uid: "u-q5".into(),
        calendar: "CHEM 999 - Lectures".into(),
        date: "2026-10-29".into(),
        time: Some("18:00-19:00".into()),
        summary: "Quiz 5 due".into(),
    }];
    let facts = daily::morning_facts(&empty_dir("mfc"), &evs, None, d(2026, 10, 28));
    assert!(facts.contains("- Quiz 5 due (CHEM 999 - Lectures), 2026-10-29"), "{facts}");
    assert_eq!(facts.lines().last(), Some("Can't slip: Quiz 5 due. Biggest free gap: 08:00-22:00."), "{facts}");
}

// A failed read: the model's reply is not sent; the unread facts stand (#138 MF-E).
#[test]
fn failed_read_dm_ignores_the_model() {
    let _g = lock();
    let deps = Deps {
        todo_dir: empty_dir("mfe"),
        cal: Some(cal_db(CHEM, PathBuf::from("/nonexistent/Calendar.sqlitedb"))),
        chat: Arc::new(Fixed("Tomorrow first: nothing on the calendar.\nTodo: nothing open\nWhich ones did you finish?".into())),
    };
    let text = daily::compose(&deps, Slot::Night, d(2026, 10, 28));
    assert!(text.contains("unknown, the calendar didn't load"), "{text}");
    assert!(!text.contains("nothing on the calendar"), "{text}");
}
