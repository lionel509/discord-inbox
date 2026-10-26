//! Breaker round 5 against 9b293bf: fenced markers, damaged-path recovery, replays under damage.
use discord_inbox::daylog::{self, Meal, Written};
use discord_inbox::nutrition::Macros;
use std::fs;
use std::path::{Path, PathBuf};

const DAY: &str = "2026-10-26";
const LOW: u32 = 1800;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-break-r5-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn meal(id: u64, kcal: f64) -> Meal {
    Meal {
        msg_id: id,
        time: "12:31".into(),
        lines: vec!["100 g salmon".into()],
        macros: Macros { kcal, protein: 20.0, carbs: 0.0, fat: 10.0 },
        veg: false,
        ..Default::default()
    }
}

fn w(dir: &Path, id: u64, kcal: f64) -> Written {
    daylog::write_meal(dir, DAY, &meal(id, kcal), LOW).unwrap()
}

fn file(dir: &Path) -> PathBuf {
    dir.join(format!("{DAY}.md"))
}

fn read(dir: &Path) -> String {
    fs::read_to_string(file(dir)).unwrap_or_default()
}

/// Probe 1: a code-fenced sample of a meal region (user documenting the format) makes logged() true
/// for an id that was never logged, so the real meal 5 is silently dropped.
#[test]
fn fenced_meal_marker_drops_real_meal() {
    let dir = scratch("fence-meal");
    let sample = "# 2026-10-26\n\n```\n<!-- pip:meal 5 -->\n- macros: 100 kcal P 1 g C 1 g F 1 g veg no\n<!-- /pip:meal 5 -->\n```\n";
    fs::write(file(&dir), sample).unwrap();
    assert!(!daylog::logged(&dir, DAY, 5), "fenced sample must not count as logged msg 5");
    assert_eq!(w(&dir, 5, 400.0), Written::Logged, "real msg 5 must be written");
    assert!(read(&dir).contains("### 12:31 · msg 5"), "meal 5 block missing:\n{}", read(&dir));
}

/// Probe 2: a fenced copy of the totals pair next to Pip's real pair makes totals_span see 2 opens and
/// go Damaged for good, so the real totals never update again while markers are intact.
#[test]
fn fenced_totals_pair_freezes_real_totals() {
    let dir = scratch("fence-totals");
    assert_eq!(w(&dir, 1, 500.0), Written::Logged);
    let mut t = read(&dir);
    t.push_str("\n```\n<!-- pip:totals -->\n**Total:** 0 kcal\n<!-- /pip:totals -->\n```\n");
    fs::write(file(&dir), t).unwrap();
    assert_eq!(w(&dir, 2, 300.0), Written::Logged);
    let text = read(&dir);
    assert!(text.contains("**Total:** 800 kcal"), "real totals not refreshed:\n{text}");
}

/// Probe 3: a fenced meal region is counted by summarize, so day totals include food never logged.
#[test]
fn fenced_meal_region_counted_in_totals() {
    let dir = scratch("fence-count");
    assert_eq!(w(&dir, 1, 500.0), Written::Logged);
    let mut t = read(&dir);
    t.push_str("\n```\n<!-- pip:meal 9 -->\n### 08:00 · msg 9\n- macros: 900 kcal P 0 g C 0 g F 0 g veg yes\n<!-- /pip:meal 9 -->\n```\n");
    fs::write(file(&dir), t).unwrap();
    let tot = daylog::totals(&dir, DAY, LOW).unwrap();
    assert_eq!(tot.meals, 1, "fenced region counted as a meal");
    assert_eq!(tot.macros.kcal, 500.0, "fenced 900 kcal counted in totals");
}

