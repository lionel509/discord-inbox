// School-calendar check-off (#95): trigger, config, scripts, parsing, the pick, and the write path. Real Calendar.app is never called: every test runs the stub.
use discord_inbox::calendar::{self, Cal, Event, Finish, Pick};
use discord_inbox::chat::Chat;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once};

// The stub log is one file per test binary. Tests take this lock so each one sees only its own lines.
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

fn guard() -> MutexGuard<'static, ()> {
    INIT.call_once(|| {
        let p = std::env::temp_dir().join(format!("di-calendar-log-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&p);
        std::env::set_var("DISCORD_INBOX_CALENDAR_LOG", &p);
    });
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn log_lines() -> Vec<String> {
    let p = std::env::var_os("DISCORD_INBOX_CALENDAR_LOG").map(PathBuf::from).unwrap();
    std::fs::read_to_string(p).unwrap_or_default().lines().map(String::from).collect()
}

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/calendar_stub.sh")
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calendar.sqlitedb")
}

fn cal(names: &[&str]) -> Cal {
    Cal { osascript: stub(), calendars: names.iter().map(|s| s.to_string()).collect(), db: fixture() }
}

fn both() -> Cal {
    cal(&["MATH 101 - Work", "PHYS 102 - Exams"])
}

// The stub's fixture rows: (uid, calendar). Home is a personal calendar the stub also holds.
const ROWS: &[(&str, &str)] = &[
    ("u-math-hw3", "MATH 101 - Work"),
    ("u-math-hw2", "MATH 101 - Work"),
    ("u-phys-mid", "PHYS 102 - Exams"),
    ("u-phys-lab", "PHYS 102 - Exams"),
    ("u-home-1", "Home"),
];

/// A chat that always answers `0`.
struct Fixed(String);
impl Chat for Fixed {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

#[test]
fn wanted_prefixes_and_suffixes() {
    assert_eq!(calendar::wanted("done with math hw 3").as_deref(), Some("math hw 3"));
    assert_eq!(calendar::wanted("I'm done with ams 301 hw 5").as_deref(), Some("ams 301 hw 5"));
    assert_eq!(calendar::wanted("finished the seltzer run").as_deref(), Some("the seltzer run"));
    assert_eq!(calendar::wanted("submitted lab 2").as_deref(), Some("lab 2"));
    assert_eq!(calendar::wanted("HW 3 done").as_deref(), Some("HW 3"));
    assert_eq!(calendar::wanted("math hw 3 ✅").as_deref(), Some("math hw 3"));
    for no in ["done", "finished", "what's due?", "bought seltzer", "done with ", "I finished"] {
        assert_eq!(calendar::wanted(no), None, "{no:?}");
    }
}

#[test]
fn parse_calendars_skips_comments_blanks_bom_and_dups() {
    let text = "\u{feff}\u{feff}# school calendars\n\nMATH 101 - Work\n  PHYS 102 - Exams  \nMATH 101 - Work\n# Home\n";
    assert_eq!(calendar::parse_calendars(text), vec!["MATH 101 - Work", "PHYS 102 - Exams"]);
    assert!(calendar::parse_calendars("").is_empty());
}

#[test]
fn missing_or_empty_calendars_file_means_feature_off() {
    let _g = guard();
    assert_eq!(Cal::load(Path::new("/nonexistent/discord-inbox/calendars")), None);
    let p = std::env::temp_dir().join(format!("di-calendars-empty-{}.txt", std::process::id()));
    std::fs::write(&p, "# nothing yet\n\n").unwrap();
    assert_eq!(Cal::load(&p), None);
}

#[test]
fn scripts_never_delete_move_or_create() {
    for s in [calendar::LIST_SCRIPT, calendar::CHECK_SCRIPT] {
        let l = s.to_lowercase();
        assert!(!l.contains("delete") && !l.contains("make new") && !l.contains("move "), "{s}");
    }
    assert!(calendar::CHECK_SCRIPT.contains("first event whose uid is"), "{}", calendar::CHECK_SCRIPT);
    assert!(calendar::CHECK_SCRIPT.contains("\"✅ \" & s"), "{}", calendar::CHECK_SCRIPT);
}

#[test]
fn parse_events_five_fields() {
    let out = "u1\tMATH 101 - Work\t2026-10-20\tallday\tHW\tpart two\nbad line\nu2\tPHYS 102 - Exams\t2026-10-21\t09:30\tMidterm 1\nu3\tX\t2026-10-22\tfour fields only\n";
    let evs = calendar::parse_events(out);
    assert_eq!(evs.len(), 2, "{evs:?}");
    assert_eq!(evs[0].summary, "HW\tpart two");
    assert_eq!(evs[0].time, None);
    assert_eq!(evs[1].uid, "u2");
    assert_eq!(evs[1].calendar, "PHYS 102 - Exams");
    assert_eq!(evs[1].date, "2026-10-21");
    assert_eq!(evs[1].time, Some("09:30".to_string()));
    assert_eq!(evs[1].summary, "Midterm 1");
}

#[test]
fn list_script_emits_time() {
    assert!(calendar::LIST_SCRIPT.contains("time of d"), "{}", calendar::LIST_SCRIPT);
    assert!(calendar::LIST_SCRIPT.contains("allday event of ev"), "{}", calendar::LIST_SCRIPT);
}

#[test]
fn done_is_the_check_mark_prefix() {
    let ev = |s: &str| Event { uid: "u".into(), calendar: "c".into(), date: "2026-10-20".into(), time: None, summary: s.into() };
    assert!(ev("✅ HW 2 due").done());
    assert!(ev("✅HW 2").done());
    assert!(!ev("HW 3 due").done());
}

#[test]
fn candidates_drops_done_and_sorts_by_distance() {
    let _g = guard();
    let events = calendar::list(&both()).unwrap();
    let open = calendar::candidates(&events, "2026-10-19");
    let names: Vec<&str> = open.iter().map(|e| e.summary.as_str()).collect();
    assert_eq!(names, vec!["HW 3 due", "Midterm 1", "Lab 2 report due"]);
}

#[test]
fn pick_prompt_says_none_when_unsure() {
    let open = vec![Event { uid: "u-math-hw3".into(), calendar: "MATH 101 - Work".into(), date: "2026-10-20".into(), time: None, summary: "HW 3 due".into() }];
    let (system, user) = calendar::pick_prompt("math hw 3", "2026-10-19", &open);
    assert!(system.contains("NONE"), "{system}");
    assert!(user.contains("#1") && user.contains("HW 3 due"), "{user}");
}

#[test]
fn parse_pick_cases() {
    assert_eq!(calendar::parse_pick("#2", 3), Pick::One(1));
    assert_eq!(calendar::parse_pick("Sure! #2 is it.", 3), Pick::One(1));
    assert_eq!(calendar::parse_pick("#2 #2", 3), Pick::One(1));
    assert_eq!(calendar::parse_pick("#1 #3", 3), Pick::Many(vec![0, 2]));
    assert_eq!(calendar::parse_pick("NONE", 3), Pick::None);
    assert_eq!(calendar::parse_pick("#9", 3), Pick::None);
    assert_eq!(calendar::parse_pick("", 3), Pick::None);
}

#[test]
fn check_off_marks_once_and_returns_the_summary() {
    let _g = guard();
    let ev = Event { uid: "u-math-hw3".into(), calendar: "MATH 101 - Work".into(), date: "2026-10-20".into(), time: None, summary: "HW 3 due".into() };
    assert_eq!(calendar::check_off(&both(), &ev).as_deref(), Ok("✅ HW 3 due"));
    assert!(log_lines().iter().any(|l| l == "check\tMATH 101 - Work\tu-math-hw3"));
}

#[test]
fn check_off_requires_confirmation() {
    let _g = guard();
    let ev = Event { uid: "u-nope".into(), calendar: "MATH 101 - Work".into(), date: "2026-10-20".into(), time: None, summary: "x".into() };
    assert!(calendar::check_off(&both(), &ev).is_err());
}

#[test]
fn finish_done_checks_off_the_picked_event() {
    let _g = guard();
    let before = log_lines().len();
    let out = calendar::finish(&Fixed("#1".into()), &both(), "math hw 3", "2026-10-19").unwrap();
    let Finish::Done(text) = out else { panic!("{out:?}") };
    assert!(text.starts_with("Checked off ✅ HW 3 due"), "{text}");
    let fresh = log_lines().split_off(before);
    assert!(fresh.iter().any(|l| l == "check\tMATH 101 - Work\tu-math-hw3"), "{fresh:?}");
}

#[test]
fn finish_none_falls_to_nomatch() {
    let _g = guard();
    let before = log_lines().len();
    let out = calendar::finish(&Fixed("NONE".into()), &both(), "the seltzer run", "2026-10-19").unwrap();
    assert_eq!(out, Finish::NoMatch);
    assert!(!log_lines().split_off(before).iter().any(|l| l.starts_with("check")));
}

#[test]
fn finish_many_asks_without_writing() {
    let _g = guard();
    let before = log_lines().len();
    let out = calendar::finish(&Fixed("#1 #2".into()), &both(), "phys", "2026-10-19").unwrap();
    let Finish::Ask(text) = out else { panic!("{out:?}") };
    assert!(text.contains("HW 3 due") && text.contains("Midterm 1") && text.contains("Which one"), "{text}");
    assert!(!log_lines().split_off(before).iter().any(|l| l.starts_with("check")));
}

#[test]
fn finish_stub_failure_is_err() {
    let _g = guard();
    let broken = Cal { osascript: PathBuf::from("/nonexistent/osascript"), calendars: vec!["MATH 101 - Work".into()], db: fixture() };
    assert!(calendar::finish(&Fixed("#1".into()), &broken, "math hw 3", "2026-10-19").is_err());
}

#[test]
fn only_configured_calendars_ever_reach_osascript() {
    let _g = guard();
    let words = ["math", "hw", "3", "phys", "mid", "lab", "home", "seltzer", "2"];
    let answers = ["#1", "NONE", "#1 #2"];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |n: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    for i in 0..200u64 {
        let mut names: Vec<String> = Vec::new();
        if next(2) == 1 {
            names.push("MATH 101 - Work".into());
        }
        if next(2) == 1 {
            names.push("PHYS 102 - Exams".into());
        }
        // A tag no other test uses, so this iteration's lines are identifiable.
        names.push(format!("Cal-{i}"));
        let c = Cal { osascript: stub(), calendars: names.clone(), db: fixture() };
        let n_words = 1 + next(3);
        let task: Vec<&str> = (0..n_words).map(|_| words[next(words.len() as u64) as usize]).collect();
        let answer = answers[next(3) as usize];
        let before = log_lines().len();
        let _ = calendar::finish(&Fixed(answer.into()), &c, &task.join(" "), "2026-10-19");
        for line in log_lines().split_off(before) {
            let f: Vec<&str> = line.split('\t').collect();
            match f[0] {
                "list" => {
                    for n in &f[1..] {
                        assert!(names.iter().any(|x| x == n), "iteration {i}: {n:?} is not configured: {line}");
                    }
                }
                "check" => {
                    assert!(names.iter().any(|x| x == f[1]), "iteration {i}: {line}");
                    assert!(ROWS.contains(&(f[2], f[1])), "iteration {i}: no such event: {line}");
                }
                _ => panic!("iteration {i}: unexpected osascript call: {line}"),
            }
            assert!(!f[1..].iter().any(|x| *x == "Home" || *x == "Work"), "iteration {i}: {line}");
        }
    }
}

#[test]
fn negated_or_qualified_done_is_not_a_finish() {
    for no in ["HW 3 not done", "lab 2 not done", "the steak was well done", "hw 3 almost done", "hw 3 nearly done", "hw 3 isn't done", "not done", "still not done with the seltzer run"] {
        assert_eq!(calendar::wanted(no), None, "{no:?}");
    }
}

#[test]
fn parse_calendars_keeps_hash_inside_a_name() {
    let names = calendar::parse_calendars("MATH 101 #2\nPHYS 102 - Exams\n# whole-line comment\n");
    assert_eq!(names, vec!["MATH 101 #2", "PHYS 102 - Exams"]);
}

#[test]
fn parse_pick_none_beats_a_mentioned_tag() {
    assert_eq!(calendar::parse_pick("NONE. #2 is closest but it's a midterm", 3), Pick::None);
    assert_eq!(calendar::parse_pick("none, not #1", 3), Pick::None);
}

#[test]
fn wanted_rejects_negated_finishes() {
    for m in [
        "HW 3 isn\u{2019}t done", "math hw 3 wasn\u{2019}t done", "HW 3 isnt done", "HW 3 arent done", "lab 2 havent done",
        "HW 3 hasn't been done", "HW 3 not quite done", "HW 3 hardly done", "HW 3 nowhere near done", "HW 3 mostly done",
        "lab 2 partly done",
    ] {
        assert_eq!(discord_inbox::calendar::wanted(m), None, "{m}");
    }
}

#[test]
fn wanted_positive_guards() {
    use discord_inbox::calendar::wanted;
    assert_eq!(wanted("HW 3 done").as_deref(), Some("HW 3"));
    assert_eq!(wanted("math assignment done").as_deref(), Some("math assignment"));
    assert_eq!(wanted("I'm done with math hw 3").as_deref(), Some("math hw 3"));
    assert_eq!(wanted("I\u{2019}m done with math hw 3").as_deref(), Some("math hw 3"));
}
