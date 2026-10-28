// Daily brief (#135): the slot clock, morning and night facts, the voice pass, and the check-in answer. The model is faked; no network, no calendar.
use chrono::{Days, NaiveDate, NaiveDateTime, NaiveTime};
use discord_inbox::calendar::Event;
use discord_inbox::chat::{Chat, NoChat};
use discord_inbox::daily::{self, Deps, Slot, Times};
use discord_inbox::todo;
use discord_inbox::weather::{self, Weather};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Fake(String);
impl Chat for Fake {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

struct Down;
impl Chat for Down {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Err("model down".to_string())
    }
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
}

fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

fn at(date: NaiveDate, h: u32, m: u32) -> NaiveDateTime {
    date.and_time(hm(h, m))
}

fn times() -> Times {
    Times { morning: hm(8, 0), night: hm(22, 30) }
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("di-daily-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn ev(uid: &str, cal: &str, date: &str, time: Option<&str>, summary: &str) -> Event {
    Event { uid: uid.into(), calendar: cal.into(), date: date.into(), time: time.map(String::from), summary: summary.into() }
}

fn week() -> Vec<Event> {
    vec![
        ev("u1", "MATH 101 - Work", "2026-10-20", None, "HW 3 due"),
        ev("u2", "PHYS 102 - Lectures", "2026-10-20", Some("14:00"), "Lecture 9"),
        ev("u3", "CHEM 101 - Work", "2026-10-20", Some("09:30"), "Lab check-in"),
        ev("u4", "BIO 101 - Lectures", "2026-10-21", Some("10:00"), "Lecture 10"),
        ev("u5", "MATH 101 - Work", "2026-10-23", None, "HW 4 due"),
        ev("u6", "MATH 101 - Work", "2026-10-24", None, "HW 5 due"),
    ]
}

fn weather_now() -> Weather {
    Weather { now_f: 52, hi_f: 61, lo_f: 45, code: 61 }
}

fn todo_fixture(name: &str) -> PathBuf {
    let dir = tmp(name);
    fs::write(dir.join("2026-10-20.md"), "---\ntags:\n  - todo\n---\nHub: [[Home]]\n- email the TA\n- read chapter 4\n").unwrap();
    dir
}

fn deps(dir: &Path, chat: Arc<dyn Chat>) -> Deps {
    Deps { todo_dir: dir.to_path_buf(), cal: None, chat }
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out: Vec<(PathBuf, Vec<u8>)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let b = fs::read(&p).unwrap();
            (p, b)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn parse_time_reads_hh_mm_only() {
    assert_eq!(daily::parse_time("08:00"), Some(hm(8, 0)));
    assert_eq!(daily::parse_time("22:30"), Some(hm(22, 30)));
    assert_eq!(daily::parse_time("8am"), None);
    assert_eq!(daily::parse_time("25:00"), None);
}

#[test]
fn times_from_env_defaults_and_overrides() {
    std::env::remove_var("DISCORD_INBOX_MORNING_AT");
    std::env::remove_var("DISCORD_INBOX_NIGHT_AT");
    assert_eq!(daily::times_from_env(), times());
    std::env::set_var("DISCORD_INBOX_MORNING_AT", "07:15");
    std::env::set_var("DISCORD_INBOX_NIGHT_AT", "nonsense");
    assert_eq!(daily::times_from_env(), Times { morning: hm(7, 15), night: hm(22, 30) });
    std::env::remove_var("DISCORD_INBOX_MORNING_AT");
    std::env::remove_var("DISCORD_INBOX_NIGHT_AT");
}

#[test]
fn due_follows_the_clock_and_the_markers() {
    let inbox = tmp("due");
    let d = day();
    let s = "2026-10-20";
    let t = times();
    assert_eq!(daily::due(&inbox, &t, at(d, 7, 59)), None);
    assert_eq!(daily::due(&inbox, &t, at(d, 8, 0)), Some((Slot::Morning, s.to_string())));
    assert_eq!(daily::due(&inbox, &t, at(d, 9, 29)), Some((Slot::Morning, s.to_string())));
    assert_eq!(daily::due(&inbox, &t, at(d, 9, 30)), None);
    daily::stage(&inbox, Slot::Morning, s).unwrap();
    assert_eq!(daily::due(&inbox, &t, at(d, 8, 30)), None);
    assert_eq!(daily::due(&inbox, &t, at(d, 22, 30)), Some((Slot::Night, s.to_string())));
    assert_eq!(daily::due(&inbox, &t, at(d, 23, 59)), Some((Slot::Night, s.to_string())));
    let next = d.checked_add_days(Days::new(1)).unwrap();
    assert_eq!(daily::due(&inbox, &t, at(next, 0, 10)), None);
}

#[test]
fn awaiting_finds_the_unanswered_night_only() {
    let inbox = tmp("await");
    let next = day().checked_add_days(Days::new(1)).unwrap();
    daily::stage(&inbox, Slot::Night, "2026-10-20").unwrap();
    assert_eq!(daily::awaiting(&inbox, at(next, 0, 30)), Some("2026-10-20".to_string()));
    daily::answered(&inbox, "2026-10-20").unwrap();
    assert_eq!(daily::awaiting(&inbox, at(next, 0, 30)), None);

    let stale = tmp("await-stale");
    daily::stage(&stale, Slot::Night, "2026-10-20").unwrap();
    let two_days = next.checked_add_days(Days::new(1)).unwrap();
    assert_eq!(daily::awaiting(&stale, at(two_days, 0, 30)), None);
}

#[test]
fn deadline_and_exam_rules() {
    assert!(daily::is_exam(&ev("e", "PHYS 102 - Exams", "2026-10-20", Some("09:30"), "Midterm 1")));
    assert!(!daily::is_exam(&ev("h", "MATH 101 - Work", "2026-10-20", None, "HW 3 due")));
    assert!(daily::is_deadline(&ev("h", "MATH 101 - Work", "2026-10-23", None, "HW 4 due")));
    assert!(!daily::is_deadline(&ev("l", "BIO 101 - Lectures", "2026-10-23", None, "Lecture 10")));
    assert!(!daily::is_deadline(&ev("d", "MATH 101 - Work", "2026-10-23", None, "✅ HW 2 due")));
}

#[test]
fn morning_facts_lays_out_weather_wear_today_and_due_soon() {
    let dir = todo_fixture("morning");
    let facts = daily::morning_facts(&dir, &week(), Some(&weather_now()), day());
    let lines: Vec<&str> = facts.lines().collect();
    assert_eq!(lines[0], "Weather (Stony Brook): 52°F now, high 61 / low 45, rain", "{facts}");
    assert_eq!(lines[1], "Wear: jacket", "{facts}");
    assert_eq!(lines[2], "Bring: nothing extra", "{facts}");
    let pos = |s: &str| facts.find(s).unwrap_or_else(|| panic!("missing {s:?} in {facts}"));
    assert!(pos("- all day: HW 3 due (MATH 101 - Work)") < pos("- 09:30 Lab check-in (CHEM 101 - Work)"), "{facts}");
    assert!(pos("- 09:30 Lab check-in (CHEM 101 - Work)") < pos("- 14:00 Lecture 9 (PHYS 102 - Lectures)"), "{facts}");
    assert!(pos("- 14:00 Lecture 9 (PHYS 102 - Lectures)") < pos("- (todo) email the TA"), "{facts}");
    assert!(pos("- (todo) email the TA") < pos("Due soon:"), "{facts}");
    assert!(facts.contains("HW 4 due"), "{facts}");
    assert!(!facts.contains("HW 5 due"), "{facts}");
    assert!(!facts.contains("Lecture 10"), "lectures are not deadlines: {facts}");
    assert!(!facts.contains("umbrella"), "{facts}");
}

#[test]
fn morning_facts_without_weather_or_list() {
    let empty = tmp("morning-empty");
    let facts = daily::morning_facts(&empty, &[], None, day());
    assert!(facts.starts_with("Weather: unavailable"), "{facts}");
    assert!(facts.contains("Today: nothing on the list"), "{facts}");
}

#[test]
fn bring_names_the_calculator_on_exam_day() {
    let dir = todo_fixture("exam");
    let exam = vec![ev("e", "PHYS 102 - Exams", "2026-10-20", Some("09:30"), "Midterm 1")];
    let facts = daily::morning_facts(&dir, &exam, Some(&weather_now()), day());
    assert!(facts.contains("Bring: calculator (exam today)"), "{facts}");
}

#[test]
fn night_facts_previews_tomorrow_and_numbers_the_list() {
    let dir = todo_fixture("night");
    fs::write(dir.join("Lab note.md"), "lab notes\n").unwrap();
    let mut evs = week();
    evs.push(ev("u8", "MATH 101 - Work", "2026-10-21", None, "Quiz 2 due"));
    let facts = daily::night_facts(&dir, &evs, day());
    assert!(facts.starts_with("Tomorrow first: 10:00 Lecture 10 (BIO 101 - Lectures)"), "{facts}");
    assert!(facts.contains("- Quiz 2 due (MATH 101 - Work)"), "{facts}");
    assert!(facts.contains("1. email the TA"), "{facts}");
    assert!(facts.contains("3. Lab note"), "{facts}");
}

#[test]
fn voice_keeps_the_facts_when_the_model_is_silent_or_short() {
    let facts = "Weather line\nWear: jacket\nBring: nothing extra";
    assert_eq!(daily::voice(&NoChat, Slot::Morning, facts), facts);
    assert_eq!(daily::voice(&Fake("ok".into()), Slot::Morning, facts), facts);
    assert_eq!(daily::voice(&Down, Slot::Night, facts), facts);
    let long = format!("Morning, Lionel! {facts}\nGood luck today 🐾");
    assert_eq!(daily::voice(&Fake(long.clone()), Slot::Morning, facts), long);
}

#[test]
fn compose_caps_the_dm_length() {
    std::env::set_var("DISCORD_INBOX_WEATHER_FILE", PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/weather_sample.json"));
    let dir = todo_fixture("compose");
    let d = deps(&dir, Arc::new(Fake("x".repeat(5000))));
    let text = daily::compose(&d, Slot::Morning, day());
    assert!(!text.is_empty());
    assert!(text.chars().count() <= daily::MAX_CHARS, "{}", text.chars().count());
}

#[test]
fn parse_answer_drops_out_of_range_numbers() {
    let a = daily::parse_answer("```json\n{\"checkin\": true, \"done\": [1, 1, 9, 0], \"tomorrow\": [\"x\"]}\n```", 2).expect("parses");
    assert!(a.checkin);
    assert_eq!(a.done, vec![1]);
    assert_eq!(a.tomorrow, vec!["x".to_string()]);
    assert_eq!(daily::parse_answer("not json", 2), None);
}

#[test]
fn answer_checks_off_and_carries_the_rest() {
    let dir = todo_fixture("answer");
    let chat = Arc::new(Fake(r#"{"checkin":true,"done":[1],"tomorrow":["study for the quiz"]}"#.into()));
    let got = daily::answer(&deps(&dir, chat), day(), "did the email, tomorrow i need to study for the quiz").unwrap();
    let text = got.expect("a check-in reply");
    assert!(text.contains("email the TA"), "{text}");
    assert!(text.contains("study for the quiz"), "{text}");
    let today = fs::read_to_string(dir.join("2026-10-20.md")).unwrap();
    assert!(today.contains("~~email the TA~~"), "{today}");
    let tomorrow = fs::read_to_string(dir.join("2026-10-21.md")).unwrap();
    assert!(tomorrow.contains("read chapter 4") && tomorrow.contains("study for the quiz"), "{tomorrow}");
    assert!(!tomorrow.contains("email the TA"), "{tomorrow}");
    let log = fs::read_to_string(dir.join(todo::CHECKIN_LOG)).unwrap();
    assert!(log.contains("## 2026-10-20"), "{log}");
}

#[test]
fn answer_that_is_not_a_checkin_writes_nothing() {
    let dir = todo_fixture("answer-off");
    let before = snapshot(&dir);
    let chat = Arc::new(Fake(r#"{"checkin":false,"done":[],"tomorrow":[]}"#.into()));
    let got = daily::answer(&deps(&dir, chat), day(), "what's the weather like").unwrap();
    assert_eq!(got, None);
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn answer_without_a_model_still_logs_the_reply() {
    let dir = todo_fixture("answer-nochat");
    let before = snapshot(&dir);
    let got = daily::answer(&deps(&dir, Arc::new(NoChat)), day(), "finished the email").unwrap();
    assert!(got.is_some());
    let log = fs::read_to_string(dir.join(todo::CHECKIN_LOG)).unwrap();
    assert!(log.contains("finished the email"), "{log}");
    for (p, b) in &before {
        assert_eq!(&fs::read(p).unwrap(), b, "{p:?} changed");
    }
    assert!(!dir.join("2026-10-21.md").exists());
}
