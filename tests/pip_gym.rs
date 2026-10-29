//! U1: the gym session logic (plan reading, rotation, reply grammar, the open-session state machine).
//! Pure except where a scratch dir is passed. The plan text is a made-up fixture, not the real plan.
use chrono::{DateTime, Duration, NaiveDate};
use discord_inbox::daylog;
use discord_inbox::gym::{self, Marker, Reply, Session, Turn};
use std::fs;
use std::path::{Path, PathBuf};

const PLAN: &str = "---
start: 2026-10-12
badminton: Tue Thu
---
# Weeks 1-2
## Day 1
- Dumbbell row
- Reverse lunges or Split squats
- Plank

## Day 2
- Leg press
- Lat pulldown
- Crunch

# Week 3+
## Push
- Bench press
- Overhead press or Dumbbell press
- Triceps pushdown

## Pull
- Cable row
- Face pull

## Legs
- Goblet squat
- Reverse lunges or Split squats

## Any day
- Hanging knee raise

| Dumbbell row | Cable row |
";

const RECEIVED: &str = "2026-10-12T18:00:00-04:00";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-gym-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn write_plan(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(gym::PLAN_FILE), PLAN).unwrap();
}

/// Mirrors what `pip::run_gym` does with a turn, minus Discord.
fn apply(dir: &Path, msg_id: u64, t: &Turn) {
    if !t.lines.is_empty() {
        daylog::write_gym(dir, &t.day, msg_id, "18:00", &t.tag, &t.lines).expect("write gym block");
    }
    match &t.marker {
        Marker::Open(s) => gym::write_marker(dir, s).expect("write marker"),
        Marker::Clear => gym::clear_marker(dir).expect("clear marker"),
        Marker::Keep => {}
    }
}

fn at(base: &str, minutes: i64) -> String {
    let t = DateTime::parse_from_rfc3339(base).unwrap() + Duration::minutes(minutes);
    t.to_rfc3339()
}

#[test]
fn exercises_reads_options_swaps_and_any_day() {
    let day1 = gym::exercises(PLAN, Session::Day1, 1);
    let names: Vec<&str> = day1.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Dumbbell row", "Reverse lunges or Split squats", "Plank"]);
    assert_eq!(day1[0].options, vec!["Dumbbell row", "Cable row"], "swap table adds the swap target");
    assert_eq!(day1[1].options, vec!["Reverse lunges", "Split squats"], "A or B splits into options");
    assert!(day1.iter().all(|e| e.name != "Hanging knee raise"), "any-day lines only from week 3");

    let push3 = gym::exercises(PLAN, Session::Push, 3);
    assert_eq!(push3.last().map(|e| e.name.as_str()), Some("Hanging knee raise"), "any-day lines follow week 3+ sections");
}

#[test]
fn exercises_empty_when_heading_missing() {
    let no_legs = "# Weeks 1-2\n## Day 1\n- Plank\n";
    assert!(gym::exercises(no_legs, Session::Legs, 3).is_empty());
}

#[test]
fn parse_reply_grammar() {
    let opts = vec!["Cable row".to_string(), "Dumbbell row".to_string(), "Plank".to_string()];

    match gym::parse_reply("cable row 15 15 12", &opts) {
        Reply::Did { option, reps } => {
            assert_eq!(option.as_deref(), Some("Cable row"));
            assert_eq!(reps, vec![15, 15, 12]);
        }
        other => panic!("expected Did, got {other:?}"),
    }
    match gym::parse_reply("Cable Row 15 15 12", &opts) {
        Reply::Did { option, .. } => assert_eq!(option.as_deref(), Some("Cable row"), "match is case-insensitive"),
        other => panic!("expected Did, got {other:?}"),
    }
    match gym::parse_reply("15 14", &opts) {
        Reply::Did { option, reps } => {
            assert_eq!(option, None);
            assert_eq!(reps, vec![15, 14]);
        }
        other => panic!("expected Did with no option, got {other:?}"),
    }
    match gym::parse_reply("3x15", &opts) {
        Reply::Did { reps, .. } => assert_eq!(reps, vec![15, 15, 15], "NxM expands to M repeated N times"),
        other => panic!("expected Did, got {other:?}"),
    }
    match gym::parse_reply("row 15", &opts) {
        Reply::Ambiguous(ms) => assert_eq!(ms, vec!["Cable row".to_string(), "Dumbbell row".to_string()]),
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    match gym::parse_reply("db row 12 12", &opts) {
        Reply::Did { option, reps } => {
            assert_eq!(option.as_deref(), Some("db row"), "off-plan pick is logged as typed");
            assert_eq!(reps, vec![12, 12]);
        }
        other => panic!("expected Did, got {other:?}"),
    }
    assert!(matches!(gym::parse_reply("felt great", &opts), Reply::Unreadable));
    assert!(matches!(gym::parse_reply("skip", &opts), Reply::Skip));
    assert!(matches!(gym::parse_reply("next", &opts), Reply::Skip));
    assert!(matches!(gym::parse_reply("/skip", &opts), Reply::Skip), "a leading slash is stripped");
    for word in ["stop", "done", "end", "quit"] {
        assert!(matches!(gym::parse_reply(word, &opts), Reply::Stop), "{word} stops");
    }
}

#[test]
fn next_session_rotates_and_defers_legs() {
    assert_eq!(gym::next_session(1, None, false), Session::Day1);
    assert_eq!(gym::next_session(1, Some(Session::Day1), false), Session::Day2);
    assert_eq!(gym::next_session(2, Some(Session::Day2), false), Session::Day1);
    assert_eq!(gym::next_session(3, None, false), Session::Push);
    assert_eq!(gym::next_session(3, Some(Session::Push), false), Session::Pull);
    assert_eq!(gym::next_session(3, Some(Session::Pull), false), Session::Legs);
    assert_eq!(gym::next_session(3, Some(Session::Legs), false), Session::Push);
    assert_eq!(gym::next_session(3, Some(Session::Pull), true), Session::Push, "no legs the day before badminton");
}

#[test]
fn week_number_and_sets_reps() {
    let start = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(); // a Monday
    let d = |day: u32| NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
    assert_eq!(gym::week_number(start, d(12)), 1);
    assert_eq!(gym::week_number(start, d(18)), 1, "the Sunday closes week 1");
    assert_eq!(gym::week_number(start, d(19)), 2);
    assert_eq!(gym::week_number(start, d(26)), 3);
    assert_eq!(gym::week_number(start, d(1)), 1, "before the start is still week 1");

    assert_eq!(gym::sets_reps(1), "1×15");
    assert_eq!(gym::sets_reps(2), "2×15");
    assert_eq!(gym::sets_reps(3), "2–3×15–20");
    assert_eq!(gym::sessions_per_week(2), 2);
    assert_eq!(gym::sessions_per_week(3), 3);
}

#[test]
fn turn_opens_logs_and_finishes() {
    let dir = scratch("opens");
    write_plan(&dir);

    let t1 = gym::turn(&dir, RECEIVED, "gym", 1).expect("gym opens a session");
    assert!(t1.reply.contains("1/3"), "{}", t1.reply);
    assert!(matches!(t1.marker, Marker::Open(_)));
    apply(&dir, 1, &t1);

    let r2 = at(RECEIVED, 1);
    let t2 = gym::turn(&dir, &r2, "cable row 15 15 12", 2).expect("open session takes the reply");
    assert_eq!(t2.lines, vec!["Dumbbell row · Cable row · reps 15 15 12".to_string()]);
    assert!(t2.reply.contains("2/3"), "{}", t2.reply);
    assert!(matches!(t2.marker, Marker::Keep));
    apply(&dir, 2, &t2);

    let r3 = at(RECEIVED, 2);
    let t3 = gym::turn(&dir, &r3, "15 15", 3).expect("second exercise");
    assert!(t3.reply.contains("3/3"), "{}", t3.reply);
    assert!(t3.lines[0].contains("reps 15 15"), "{:?}", t3.lines);
    apply(&dir, 3, &t3);

    let r4 = at(RECEIVED, 3);
    let t4 = gym::turn(&dir, &r4, "skip", 4).expect("last exercise");
    assert!(matches!(t4.marker, Marker::Clear), "session ends after the last exercise");
    assert!(t4.reply.contains("Day 1 done"), "{}", t4.reply);
    assert!(t4.reply.contains("2 logged, 1 skipped"), "{}", t4.reply);
    apply(&dir, 4, &t4);

    assert_eq!(daylog::gym_blocks(&dir, "2026-10-12").len(), 3, "one block per logged or skipped exercise");
    assert!(gym::read_open(&dir, &at(RECEIVED, 4)).is_none(), "marker cleared");
}

#[test]
fn turn_skip_writes_a_block() {
    let dir = scratch("skip");
    write_plan(&dir);
    apply(&dir, 1, &gym::turn(&dir, RECEIVED, "gym", 1).unwrap());

    let t = gym::turn(&dir, &at(RECEIVED, 1), "skip", 2).expect("skip is a session reply");
    assert_eq!(t.lines, vec!["skipped: Dumbbell row".to_string()]);
    assert!(t.reply.contains("2/3"), "{}", t.reply);
    apply(&dir, 2, &t);

    let blocks = daylog::gym_blocks(&dir, "2026-10-12");
    assert!(blocks.iter().any(|(id, _, lines)| *id == 2 && lines.contains(&"skipped: Dumbbell row".to_string())), "{blocks:?}");
}

#[test]
fn turn_times_out_after_four_hours() {
    let dir = scratch("timeout");
    write_plan(&dir);
    apply(&dir, 1, &gym::turn(&dir, RECEIVED, "gym", 1).unwrap());

    let inside = gym::turn(&dir, &at(RECEIVED, 4 * 60 - 1), "15 15 12", 2);
    assert!(inside.is_some(), "3h59m after opening is still the session");
    apply(&dir, 2, &inside.unwrap());

    let outside = gym::turn(&dir, &at(RECEIVED, 4 * 60 + 1), "15 15 12", 3);
    assert!(outside.is_none(), "4h01m after opening falls through to the normal path");
}

#[test]
fn turn_ignores_text_received_before_open() {
    let dir = scratch("before");
    write_plan(&dir);
    apply(&dir, 1, &gym::turn(&dir, RECEIVED, "gym", 1).unwrap());
    let replay = at(RECEIVED, -1);
    assert!(gym::turn(&dir, &replay, "15 15 12", 2).is_none(), "a DM older than the open is a replay, not a reply");
}

#[test]
fn turn_missing_plan_replies_error_without_marker() {
    let dir = scratch("noplan");
    let t = gym::turn(&dir, RECEIVED, "gym", 1).expect("gym is always answered");
    assert!(t.reply.starts_with("Can't read the gym plan"), "{}", t.reply);
    assert!(t.lines.is_empty());
    assert!(matches!(t.marker, Marker::Keep));
    assert!(gym::read_open(&dir, RECEIVED).is_none());
}

#[test]
fn turn_passes_today_week_help_through_and_refuses_undo() {
    let dir = scratch("passthrough");
    write_plan(&dir);
    apply(&dir, 1, &gym::turn(&dir, RECEIVED, "gym", 1).unwrap());

    for (i, word) in ["today", "week", "help"].iter().enumerate() {
        assert!(gym::turn(&dir, &at(RECEIVED, 1 + i as i64), word, 10 + i as u64).is_none(), "{word} passes through");
    }
    for (i, word) in ["undo", "fix 100g salmon"].iter().enumerate() {
        let t = gym::turn(&dir, &at(RECEIVED, 5 + i as i64), word, 20 + i as u64).expect("refused, not passed on");
        assert!(t.reply.contains("undo and fix are for meals"), "{}", t.reply);
        assert!(t.lines.is_empty());
        assert!(matches!(t.marker, Marker::Keep));
    }
    assert!(gym::read_open(&dir, &at(RECEIVED, 10)).is_some(), "the session stays open");
}

#[test]
fn gym_again_after_finish_moves_to_next_session() {
    let dir = scratch("again");
    write_plan(&dir);
    apply(&dir, 1, &gym::turn(&dir, RECEIVED, "gym", 1).unwrap());
    for (i, id) in [2u64, 3, 4].iter().enumerate() {
        let t = gym::turn(&dir, &at(RECEIVED, 1 + i as i64), "skip", *id).expect("session reply");
        apply(&dir, *id, &t);
    }
    assert!(gym::read_open(&dir, &at(RECEIVED, 10)).is_none(), "Day 1 finished");

    let t = gym::turn(&dir, &at(RECEIVED, 60), "gym", 5).expect("gym again");
    assert!(t.reply.contains("Day 2"), "{}", t.reply);
    assert!(t.reply.contains("1/3"), "{}", t.reply);
}
