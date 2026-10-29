//! #144 round 4: the core pick, per-item targets and seconds. Pure except where a scratch dir is passed.
//! The plan text is a made-up fixture, not the real plan.
use chrono::{DateTime, Duration};
use discord_inbox::daylog;
use discord_inbox::gym::{self, Marker, Pick, Reply, Session, Turn};
use std::fs;
use std::path::{Path, PathBuf};

const PLAN: &str = "---
start: 2026-10-12
badminton: Tue Thu
---
## Weeks 1–2 (made up)
Week 1: 1 set × 15. Week 2: 2 sets × 15.

### Day 1
- Lantern lift
- Comet hold (30 s each side)

### Day 2
- Kettle sweep

## Week 3+ workouts
2–3 sets × 15–20, slow and controlled.

### Push
- Lantern lift
- Otter twist or Badger press

### Pull
- Kettle sweep

### Legs
- Heron reach (left + right foot)

### Core (end of any day, pick 2–3)
- Moth curl 2×10/side
- Comet plank 2×20 s/side (slow exhale)
- Lynx roll (30 s each side)
- Heron reach (left + right foot)
- Pike tuck
";

const RECEIVED: &str = "2026-10-26T18:00:00-04:00";
const DAY: &str = "2026-10-26";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-gym-core-{}-{name}", std::process::id()));
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

/// One message at `minutes` after RECEIVED, applied to the scratch folder.
fn say(dir: &Path, minutes: i64, text: &str, id: u64) -> Turn {
    let t = gym::turn(dir, &at(RECEIVED, minutes), text, id).unwrap_or_else(|| panic!("no gym turn for {text:?}"));
    apply(dir, id, &t);
    t
}

/// Opens Push on the made-up plan and answers up to the pick prompt: `gym`, `15 15`, `skip`.
fn to_pick(dir: &Path) -> Turn {
    write_plan(dir);
    say(dir, 0, "gym", 1);
    say(dir, 1, "15 15", 2);
    say(dir, 2, "skip", 3)
}

#[test]
fn pick_range_from_heading() {
    assert_eq!(gym::pick_range("core (end of any day, pick 2–3)"), Some((2, 3)));
    assert_eq!(gym::pick_range("core, pick 2-3"), Some((2, 3)));
    assert_eq!(gym::pick_range("core, pick 2 to 3"), Some((2, 3)));
    assert_eq!(gym::pick_range("core, pick 2—3"), Some((2, 3)));
    assert_eq!(gym::pick_range("core, pick 2"), Some((2, 2)), "pick 2 means exactly 2");
    assert_eq!(gym::pick_range("core"), None);
    assert_eq!(gym::pick_range("core, pick 3–2"), None);
    assert_eq!(gym::pick_range("core, pick 0"), None);

    assert_eq!(gym::core(PLAN, 3).range, Some((2, 3)));
    assert_eq!(gym::core(PLAN, 3).items.len(), 5);
    assert!(gym::core(PLAN, 2).items.is_empty(), "no core before week 3");
}

#[test]
fn core_pick_walks_only_picked() {
    let dir = scratch("walk");
    write_plan(&dir);
    let open = say(&dir, 0, "gym", 1);
    assert!(open.reply.contains("2 exercises + core (pick 2–3)"), "{}", open.reply);
    assert!(open.reply.contains("1/2 · Lantern lift"), "{}", open.reply);

    let answered = say(&dir, 1, "15 15", 2);
    assert_eq!(answered.lines, vec!["Lantern lift · reps 15 15".to_string()]);

    let pick = say(&dir, 2, "skip", 3);
    assert_eq!(pick.lines, vec!["skipped: Otter twist or Badger press".to_string()]);
    assert!(matches!(pick.marker, Marker::Keep));
    assert!(pick.reply.contains("pick 2–3"), "{}", pick.reply);
    assert!(pick.reply.contains("1. Moth curl · 2×10/side"), "{}", pick.reply);
    assert!(pick.reply.contains("5. Pike tuck · 2–3×15–20"), "{}", pick.reply);

    let picked = say(&dir, 3, "1 3", 4);
    assert!(picked.lines.is_empty(), "the pick writes no block");
    assert!(matches!(&picked.marker, Marker::Open(s) if s.ends_with(" core=1,3")), "{:?}", picked.marker);
    assert!(picked.reply.contains("3/4 · Moth curl · 2×10/side"), "{}", picked.reply);

    let moth = say(&dir, 4, "10 10", 5);
    assert_eq!(moth.lines, vec!["Moth curl · reps 10 10".to_string()]);
    assert!(moth.reply.contains("4/4 · Lynx roll · 30 s each side"), "{}", moth.reply);

    let last = say(&dir, 5, "30s 30s", 6);
    assert_eq!(last.lines, vec!["Lynx roll · seconds 30 30".to_string()]);
    assert!(matches!(last.marker, Marker::Clear));
    assert!(last.reply.contains("Push done · 3 logged, 1 skipped"), "{}", last.reply);

    let blocks = daylog::gym_blocks(&dir, DAY);
    assert_eq!(blocks.len(), 4, "own, skipped own, picked, picked");
    let all: String = blocks.iter().flat_map(|(_, _, l)| l.clone()).collect::<Vec<_>>().join("\n");
    for absent in ["Comet plank", "Heron reach", "Pike tuck"] {
        assert!(!all.contains(absent), "unpicked core item {absent} was logged: {all}");
    }
}

#[test]
fn parse_pick_grammar() {
    let ok = |t: &str| gym::parse_pick(t, 5, 2, 3);
    assert_eq!(ok("1 3"), Pick::Chosen(vec![1, 3]));
    assert_eq!(ok("3, 1"), Pick::Chosen(vec![3, 1]), "typed order is kept");
    assert_eq!(ok("1 and 3"), Pick::Chosen(vec![1, 3]), "and is ignored");
    assert_eq!(ok("1 & 3"), Pick::Chosen(vec![1, 3]), "& is ignored");
    assert_eq!(ok("1 1 3"), Pick::Chosen(vec![1, 3]), "duplicates keep the first");

    for word in ["skip", "next", "none", "no", "/skip"] {
        assert_eq!(ok(word), Pick::Skip, "{word} skips core");
    }
    for word in ["stop", "done", "end", "quit"] {
        assert_eq!(ok(word), Pick::Stop, "{word} stops");
    }

    for n in 1..=5 {
        assert_eq!(ok(&n.to_string()), Pick::Bad, "one pick is below lo");
    }
    assert_eq!(ok("0 1"), Pick::Bad, "0 is not an item");
    assert_eq!(ok("6 1"), Pick::Bad, "6 is past the list");
    assert_eq!(ok("abs"), Pick::Bad);
    assert_eq!(ok("1 1"), Pick::Bad, "duplicates count once");
    for omit in 1..=5 {
        let four: Vec<String> = (1..=5).filter(|&x| x != omit).map(|x| x.to_string()).collect();
        assert_eq!(ok(&four.join(" ")), Pick::Bad, "four picks is above hi");
    }
}

#[test]
fn bad_pick_reprompts_and_writes_nothing() {
    let dir = scratch("badpick");
    to_pick(&dir);
    let before = fs::read(dir.join(gym::MARKER)).unwrap();
    let t = say(&dir, 3, "7", 4);
    assert!(t.lines.is_empty());
    assert!(matches!(t.marker, Marker::Keep));
    assert!(t.reply.starts_with("Couldn't read that as a pick"), "{}", t.reply);
    assert!(t.reply.contains("1. Moth curl"), "{}", t.reply);
    assert_eq!(fs::read(dir.join(gym::MARKER)).unwrap(), before, "marker unchanged");
}

#[test]
fn skip_at_pick_finishes_without_core() {
    let dir = scratch("skippick");
    to_pick(&dir);
    let t = say(&dir, 3, "skip", 4);
    assert!(matches!(t.marker, Marker::Clear));
    assert!(t.lines.is_empty());
    assert!(t.reply.contains("Push done · 1 logged, 1 skipped"), "{}", t.reply);
    assert_eq!(daylog::gym_blocks(&dir, DAY).len(), 2);
}

#[test]
fn stop_at_pick_clears() {
    let dir = scratch("stoppick");
    to_pick(&dir);
    let t = say(&dir, 3, "stop", 4);
    assert!(matches!(t.marker, Marker::Clear));
    assert!(t.lines.is_empty());
    assert!(t.reply.starts_with("Stopped before core."), "{}", t.reply);
}

#[test]
fn every_pick_logs_exactly_the_picked_items() {
    let names = ["Moth curl", "Comet plank", "Lynx roll", "Heron reach", "Pike tuck"];
    let mut cases: Vec<Vec<usize>> = Vec::new();
    for a in 1..=5 {
        for b in 1..=5 {
            if b == a {
                continue;
            }
            cases.push(vec![a, b]);
            for c in 1..=5 {
                if c != a && c != b {
                    cases.push(vec![a, b, c]);
                }
            }
        }
    }
    assert_eq!(cases.len(), 80);
    for (i, picks) in cases.iter().enumerate() {
        let dir = scratch(&format!("every-{i}"));
        to_pick(&dir);
        let typed: Vec<String> = picks.iter().map(|p| p.to_string()).collect();
        say(&dir, 3, &typed.join(" "), 4);
        let mut expected = vec!["Lantern lift · reps 15 15".to_string(), "skipped: Otter twist or Badger press".to_string()];
        for (j, p) in picks.iter().enumerate() {
            say(&dir, 4 + j as i64, "skip", 5 + j as u64);
            expected.push(format!("skipped: {}", names[p - 1]));
        }
        let blocks = daylog::gym_blocks(&dir, DAY);
        let lines: Vec<String> = blocks.iter().flat_map(|(_, _, l)| l.clone()).collect();
        assert_eq!(lines, expected, "picks {picks:?}");
        assert_eq!(blocks.len(), 2 + picks.len(), "picks {picks:?}");
        assert!(!dir.join(gym::MARKER).exists(), "picks {picks:?}: session closed");
    }
}

#[test]
fn pick_survives_restart() {
    let dir = scratch("restart");
    to_pick(&dir);
    say(&dir, 3, "1 3", 4);
    assert_eq!(gym::read_open(&dir, &at(RECEIVED, 3)).unwrap().core, Some(vec![1, 3]));

    // A fresh `gym` after a restart reads the same state from disk.
    let t = say(&dir, 4, "gym", 5);
    assert!(t.reply.contains("3/4 · Moth curl"), "{}", t.reply);
    assert!(matches!(t.marker, Marker::Keep));
    assert!(t.lines.is_empty());
}

#[test]
fn gym_resent_at_pick_shows_pick_again() {
    let dir = scratch("resent");
    to_pick(&dir);
    let t = say(&dir, 3, "gym", 4);
    assert!(t.reply.contains("pick 2–3"), "{}", t.reply);
    assert!(matches!(t.marker, Marker::Keep));
    assert!(t.lines.is_empty());
}

#[test]
fn pick_lost_before_marker_write_reasks() {
    let dir = scratch("lost");
    to_pick(&dir);
    // A crash after the reply is read and before the marker is written: the pick is never applied.
    let _lost = gym::turn(&dir, &at(RECEIVED, 3), "1 3", 4).expect("the pick is read");
    let t = say(&dir, 4, "10 10", 5);
    assert!(t.reply.contains("pick 2–3"), "{}", t.reply);
    assert!(t.lines.is_empty(), "nothing is logged against a core item");
    assert!(matches!(t.marker, Marker::Keep));
}

#[test]
fn marker_core_token_round_trip() {
    let dir = scratch("marker");
    let opened = "2026-10-26T18:00:00-04:00";
    gym::write_marker(&dir, "2026-10-26 push 3 1 2026-10-26T18:00:00-04:00").unwrap();
    assert_eq!(gym::read_open(&dir, RECEIVED).unwrap().core, None, "a marker without core= still parses");

    gym::write_marker(&dir, "2026-10-26 push 3 1 2026-10-26T18:00:00-04:00 core=1,3").unwrap();
    assert_eq!(gym::read_open(&dir, RECEIVED).unwrap().core, Some(vec![1, 3]));

    gym::write_marker(&dir, "2026-10-26 push 3 1 2026-10-26T18:00:00-04:00 core=x").unwrap();
    let bad = gym::read_open(&dir, RECEIVED).expect("still open");
    assert_eq!(bad.core, None, "an unreadable core= leaves the pick to be asked again");

    gym::write_marker(&dir, &format!("2026-10-26 day 1 3 1 {opened} core=2,1")).unwrap();
    let day1 = gym::read_open(&dir, RECEIVED).unwrap();
    assert_eq!(day1.session, Session::Day1);
    assert_eq!(day1.core, Some(vec![2, 1]));

    let o = gym::Open {
        day: "2026-10-26".into(),
        session: Session::Push,
        week: 3,
        open_id: 1,
        opened: DateTime::parse_from_rfc3339(opened).unwrap(),
        core: Some(vec![3, 1]),
    };
    gym::write_marker(&dir, &gym::marker_text(&o)).unwrap();
    assert_eq!(gym::read_open(&dir, RECEIVED), Some(o));
}

#[test]
fn item_targets_and_notes_split() {
    let items = gym::core(PLAN, 3).items;
    let moth = &items[0];
    assert_eq!(moth.name, "Moth curl");
    assert_eq!(moth.target.as_deref(), Some("2×10/side"));
    assert_eq!(moth.note, None);
    assert!(!moth.seconds);

    let comet = &items[1];
    assert_eq!(comet.name, "Comet plank");
    assert_eq!(comet.target.as_deref(), Some("2×20 s/side"));
    assert_eq!(comet.note.as_deref(), Some("slow exhale"), "a bracket without a digit is a note");
    assert!(comet.seconds);

    let lynx = &items[2];
    assert_eq!(lynx.name, "Lynx roll");
    assert_eq!(lynx.target.as_deref(), Some("30 s each side"), "a bracket with a digit is the target");
    assert_eq!(lynx.note, None);
    assert!(lynx.seconds);

    let heron = &items[3];
    assert_eq!(heron.name, "Heron reach");
    assert_eq!(heron.target, None);
    assert_eq!(heron.note.as_deref(), Some("left + right foot"));
    assert!(!heron.seconds);

    let pike = &items[4];
    assert_eq!(pike.name, "Pike tuck");
    assert_eq!(pike.target, None);

    let push = gym::exercises(PLAN, Session::Push, 3);
    assert_eq!(push[1].options, vec!["Otter twist".to_string(), "Badger press".to_string()]);
    assert_eq!(push[1].target, None);

    let one = gym::exercises("### Push\n- Moth curl 2x10/side\n", Session::Push, 3);
    assert_eq!(one[0].name, "Moth curl");
    assert_eq!(one[0].target.as_deref(), Some("2x10/side"));
}

#[test]
fn prompt_uses_item_target_over_week_default() {
    let items = gym::core(PLAN, 3).items;
    let (moth, heron, pike) = (&items[0], &items[3], &items[4]);
    let p = gym::prompt(moth, 0, 1, 3);
    assert!(p.contains("2×10/side") && !p.contains("2–3×15–20"), "{p}");
    assert!(gym::prompt(pike, 0, 1, 3).contains("2–3×15–20"), "no target falls back to the week");
    assert!(gym::prompt(heron, 0, 1, 3).contains("· left + right foot"), "the note is shown");
    assert_eq!(
        gym::parse_reply("moth curl 10 10", &moth.options),
        Reply::Did { option: Some("Moth curl".into()), reps: vec![10, 10] }
    );
}

#[test]
fn seconds_reply_grammar() {
    let lynx = vec!["Lynx roll".to_string()];
    for text in ["30 30", "30s 30s", "30 s 30 s", "2x30s", "30sec 30sec"] {
        match gym::parse_reply_unit(text, &lynx, true) {
            Reply::Did { reps, .. } => assert_eq!(reps, vec![30, 30], "{text}"),
            other => panic!("{text}: expected Did, got {other:?}"),
        }
    }
    assert_eq!(
        gym::parse_reply_unit("lynx roll 45s 40s", &lynx, true),
        Reply::Did { option: Some("Lynx roll".into()), reps: vec![45, 40] }
    );
    assert_eq!(gym::parse_reply_unit("300", &lynx, true), Reply::Did { option: None, reps: vec![300] });
    assert_eq!(gym::parse_reply_unit("700", &lynx, true), Reply::Unreadable, "seconds cap is 600");

    assert_eq!(gym::parse_reply_unit("30s", &lynx, false), Reply::Unreadable, "reps mode does not read seconds");
    assert_eq!(gym::parse_reply_unit("300", &lynx, false), Reply::Unreadable, "reps mode cap is 200");
}

#[test]
fn seconds_line_in_day_note_week1() {
    let dir = scratch("seconds");
    write_plan(&dir);
    let day = "2026-10-12";
    let base = "2026-10-12T18:00:00-04:00";
    let step = |minutes: i64, text: &str, id: u64| {
        let t = gym::turn(&dir, &at(base, minutes), text, id).expect("gym turn");
        apply(&dir, id, &t);
        t
    };
    step(0, "gym", 1);
    let prompt = step(1, "15", 2);
    assert!(prompt.reply.contains("seconds per set"), "{}", prompt.reply);
    let last = step(2, "30s 30s", 3);
    assert_eq!(last.lines, vec!["Comet hold · seconds 30 30".to_string()]);

    let note = fs::read_to_string(daylog::note_path(&dir, day)).unwrap();
    assert!(note.contains("seconds 30 30"), "{note}");
    assert!(!note.contains("reps 30 30"), "{note}");
}
