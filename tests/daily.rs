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
    assert!(daily::is_deadline(&ev("m", "SBU - Exams", "2026-10-22", None, "Midterm 1")));
    assert!(!daily::is_deadline(&ev("r", "SBU - Lectures", "2026-10-21", None, "Reading due Friday")));
    assert!(daily::is_exam(&ev("q", "MATH 131 - Work", "2026-10-20", Some("10:00"), "Quiz 3")));
    assert!(daily::is_exam(&ev("f", "MATH 131 - Work", "2026-10-20", None, "Final review")));
    assert!(!daily::is_exam(&ev("x", "PHYS - Lectures", "2026-10-20", None, "Example sheet 4")));
}

#[test]
fn morning_facts_lays_out_weather_wear_today_and_due_soon() {
    let dir = todo_fixture("morning");
    fs::write(dir.join("Call the TA.md"), "about the lab\n").unwrap();
    let facts = daily::morning_facts(&dir, &week(), Some(&weather_now()), day());
    let lines: Vec<&str> = facts.lines().collect();
    assert_eq!(lines[0], "Weather (Stony Brook): 52°F now, high 61 / low 45, rain", "{facts}");
    assert_eq!(lines[1], "Wear: jacket", "{facts}");
    assert_eq!(lines[2], "Bring: nothing extra", "{facts}");
    let pos = |s: &str| facts.find(s).unwrap_or_else(|| panic!("missing {s:?} in {facts}"));
    assert!(pos("- all day: HW 3 due (MATH 101 - Work)") < pos("- 09:30 Lab check-in (CHEM 101 - Work)"), "{facts}");
    assert!(pos("- 09:30 Lab check-in (CHEM 101 - Work)") < pos("- 14:00 Lecture 9 (PHYS 102 - Lectures)"), "{facts}");
    assert!(pos("- 14:00 Lecture 9 (PHYS 102 - Lectures)") < pos("- (todo) email the TA"), "{facts}");
    assert!(pos("- (todo) email the TA") < pos("- (open) Call the TA") && pos("- (open) Call the TA") < pos("Due soon:"), "{facts}");
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
    let got = daily::answer(&deps(&dir, chat), day(), "did the email, tomorrow i need to study for the quiz", None).unwrap();
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
    let got = daily::answer(&deps(&dir, chat), day(), "what's the weather like", None).unwrap();
    assert_eq!(got, None);
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn answer_without_a_model_writes_nothing() {
    let dir = todo_fixture("answer-nochat");
    let before = snapshot(&dir);
    let got = daily::answer(&deps(&dir, Arc::new(NoChat)), day(), "finished the email", None);
    assert!(got.is_err());
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn answer_numbers_follow_the_list_as_sent() {
    let dir = tmp("sent-list");
    fs::write(dir.join("2026-10-20.md"), "- alpha\n- beta\n- gamma\n").unwrap();
    fs::write(dir.join("Zzz.md"), "a loose note\n").unwrap();
    // The list as the night DM showed it: alpha, beta, gamma, Zzz.
    let sent: Vec<String> = todo::day_items(&dir, day()).into_iter().chain(todo::note_items(&dir)).map(|i| i.text).collect();
    assert_eq!(sent, ["alpha", "beta", "gamma", "Zzz"].map(String::from));
    // A note filed after the DM sorts before Zzz, so the live list has Zzz at 5.
    fs::write(dir.join("Aaa.md"), "- aaa\n").unwrap();
    let chat = Arc::new(Fake(r#"{"checkin":true,"done":[4],"tomorrow":[]}"#.into()));
    daily::answer(&deps(&dir, chat), day(), "did the loose note", Some(sent)).unwrap();
    let zzz = fs::read_to_string(dir.join("Zzz.md")).unwrap();
    assert!(zzz.contains("done:"), "number 4 must strike Zzz, the note it was sent for: {zzz}");
    let aaa = fs::read_to_string(dir.join("Aaa.md")).unwrap();
    assert!(!aaa.contains("done:"), "number 4 struck the note filed after the DM: {aaa}");

    // Three items were sent: a 4 is out of range and strikes nothing.
    let dir = tmp("sent-three");
    fs::write(dir.join("2026-10-20.md"), "- alpha\n- beta\n- gamma\n").unwrap();
    let sent: Vec<String> = todo::day_items(&dir, day()).into_iter().map(|i| i.text).collect();
    fs::write(dir.join("Zzz.md"), "a loose note\n").unwrap();
    let chat = Arc::new(Fake(r#"{"checkin":true,"done":[4],"tomorrow":[]}"#.into()));
    daily::answer(&deps(&dir, chat), day(), "did the loose note", Some(sent)).unwrap();
    let body = fs::read_to_string(dir.join("2026-10-20.md")).unwrap();
    assert!(!body.contains("~~"), "a number outside the sent list struck an item: {body}");
    let zzz = fs::read_to_string(dir.join("Zzz.md")).unwrap();
    assert!(!zzz.contains("done:"), "a number outside the sent list struck a note: {zzz}");
}

// Records the system prompt the voice pass was sent.
struct Record {
    reply: String,
    system: std::sync::Mutex<String>,
}
impl Chat for Record {
    fn ask(&self, system: &str, _user: &str) -> Result<String, String> {
        *self.system.lock().unwrap() = system.to_string();
        Ok(self.reply.clone())
    }
}

#[test]
fn voice_threshold_is_a_third_of_the_facts() {
    let facts = "a".repeat(300);
    // 60 characters is under a third of 300: the facts go out.
    assert_eq!(daily::voice(&Fake("b".repeat(60)), Slot::Night, &facts), facts);
    // 120 is over a third: the reply goes out.
    let reply = "b".repeat(120);
    assert_eq!(daily::voice(&Fake(reply.clone()), Slot::Night, &facts), reply);
}

#[test]
fn prompts_carry_the_guard_rails() {
    let rec = Record { reply: "ok".into(), system: std::sync::Mutex::new(String::new()) };
    daily::voice(&rec, Slot::Morning, "Weather line");
    let morning = rec.system.lock().unwrap().clone();
    assert!(morning.contains("Under 1500 characters") && morning.contains("umbrellas"), "{morning}");
    daily::voice(&rec, Slot::Night, "Weather line");
    let night = rec.system.lock().unwrap().clone();
    assert!(night.contains("Under 1500 characters"), "{night}");
    let (system, _) = daily::answer_prompt(&[], "x", day());
    assert!(system.contains("not already in the list"), "{system}");
}

#[test]
fn saved_list_round_trips_for_the_reply() {
    let inbox = tmp("list-roundtrip");
    let items = vec![todo::Item { text: "alpha".into(), place: todo::Place::Note { file: inbox.join("Alpha.md") } }];
    daily::save_list(&inbox, "2026-10-20", &items).unwrap();
    assert_eq!(daily::load_list(&inbox, "2026-10-20"), Some(vec!["alpha".to_string()]));
    assert_eq!(daily::load_list(&inbox, "2026-10-21"), None);
}
