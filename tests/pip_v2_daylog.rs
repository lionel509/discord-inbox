//! U3: note blocks that are not meals, undo by marker, `meals()` listing, and the command ledger. Scratch dirs are `…/Private/Nutrition`.
use discord_inbox::daylog::{self, Meal, NoteMsg, Written};
use discord_inbox::nutrition::Macros;
use std::fs;
use std::path::{Path, PathBuf};

const DAY: &str = "2026-10-25";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-v2-daylog-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn meal(id: u64, kcal: f64) -> Meal {
    Meal {
        msg_id: id,
        time: "12:31".into(),
        lines: vec!["- 100 g salmon · 208 kcal".into()],
        macros: Macros { kcal, protein: 20.0, carbs: 10.0, fat: 5.0 },
        veg: false,
        ..Default::default()
    }
}

fn note(id: u64, text: &str) -> NoteMsg {
    NoteMsg { msg_id: id, time: "19:30".into(), text: text.into(), photos: Vec::new() }
}

fn read(dir: &Path) -> String {
    fs::read_to_string(daylog::note_path(dir, DAY)).unwrap_or_default()
}

/// A day file a person started by hand: hand-typed lines before and after the pip markers.
fn seed(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let text = "# 2026-10-25\n\nhand line one\n\n<!-- pip:totals -->\n<!-- /pip:totals -->\n\n## Meals\n\nhand line two\n";
    fs::write(daylog::note_path(dir, DAY), text).unwrap();
}

#[test]
fn a_note_is_logged_but_is_not_a_meal() {
    let dir = scratch("note");
    seed(&dir);
    assert_eq!(daylog::write_note(&dir, DAY, &note(7, "hi pip"), 1800).unwrap(), Written::Logged);
    assert!(daylog::logged(&dir, DAY, 7));
    assert!(!daylog::has_meal(&dir, DAY));
    assert_eq!(daylog::totals(&dir, DAY, 1800).unwrap().meals, 0);
    assert_eq!(daylog::write_note(&dir, DAY, &note(7, "hi pip"), 1800).unwrap(), Written::Already);
}

#[test]
fn undo_marks_the_last_meal_and_keeps_its_bytes() {
    let dir = scratch("undo");
    seed(&dir);
    daylog::write_meal(&dir, DAY, &meal(1, 300.0), 1800).unwrap();
    daylog::write_meal(&dir, DAY, &meal(2, 500.0), 1800).unwrap();
    let before = read(&dir);
    let undone = daylog::undo_last(&dir, DAY, 1800, 9).unwrap().expect("a meal to undo");
    assert_eq!(undone.msg_id, 2);
    assert!((undone.kcal - 500.0).abs() < 0.01);
    assert_eq!(daylog::totals(&dir, DAY, 1800).unwrap().meals, 1);
    assert!(daylog::logged(&dir, DAY, 2), "an undone id still counts as logged");
    let after = read(&dir);
    assert!(after.contains("<!-- pip:undone 2 -->"));
    assert!(after.contains("<!-- /pip:undone 2 -->"));
    assert!(!after.contains("<!-- pip:meal 2 -->"));
    // Apart from the two marker lines, the inserted `- undone:` line and the totals region, nothing else changes.
    let body = |t: &str| -> Vec<String> {
        let mut inside = false;
        let mut out = Vec::new();
        for l in t.lines() {
            if l.starts_with("<!-- pip:meal 2 -->") || l.starts_with("<!-- pip:undone 2 -->") {
                inside = true;
                continue;
            }
            if l.starts_with("<!-- /pip:meal 2 -->") || l.starts_with("<!-- /pip:undone 2 -->") {
                inside = false;
                continue;
            }
            if inside && !l.starts_with("- undone:") {
                out.push(l.to_string());
            }
        }
        out
    };
    assert_eq!(body(&before), body(&after));
    assert!(before.contains("hand line two") && after.contains("hand line two"));
}

#[test]
fn undo_on_an_empty_day_is_none() {
    let dir = scratch("empty");
    seed(&dir);
    assert!(daylog::undo_last(&dir, DAY, 1800, 3).unwrap().is_none());
}

#[test]
fn meals_lists_time_kcal_and_lines() {
    let dir = scratch("list");
    seed(&dir);
    daylog::write_meal(&dir, DAY, &meal(4, 412.0), 1800).unwrap();
    let rows = daylog::meals(&dir, DAY);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].msg_id, 4);
    assert_eq!(rows[0].time, "12:31");
    assert!((rows[0].kcal - 412.0).abs() < 0.01);
    assert_eq!(rows[0].lines, vec!["100 g salmon · 208 kcal".to_string()]);
}

#[test]
fn command_ledger_round_trips() {
    let dir = scratch("ledger");
    seed(&dir);
    assert!(!daylog::command_seen(&dir, 77));
    daylog::record_command(&dir, 77).unwrap();
    assert!(daylog::command_seen(&dir, 77));
    assert!(!daylog::command_seen(&dir, 78));
}

/// Fixed-seed LCG: the same 300 sequences every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn invariant_user_bytes_survive_and_totals_track_live_meals() {
    let mut rng = Lcg(0xD41_111);
    for seq in 0..300 {
        let dir = scratch(&format!("inv{seq}"));
        seed(&dir);
        let mut next_id = 100u64;
        let mut live: Vec<(u64, f64)> = Vec::new();
        let mut ever: Vec<u64> = Vec::new();
        for _ in 0..(3 + rng.next() % 6) {
            match rng.next() % 3 {
                0 => {
                    let kcal = (50 + rng.next() % 850) as f64;
                    let id = next_id;
                    next_id += 1;
                    daylog::write_meal(&dir, DAY, &meal(id, kcal), 1800).unwrap();
                    live.push((id, kcal));
                    ever.push(id);
                }
                1 => {
                    let id = next_id;
                    next_id += 1;
                    daylog::write_note(&dir, DAY, &note(id, "test"), 1800).unwrap();
                    ever.push(id);
                }
                _ => {
                    let undone = daylog::undo_last(&dir, DAY, 1800, 1).unwrap();
                    assert_eq!(undone.is_some(), !live.is_empty(), "seq {seq}");
                    if undone.is_some() {
                        live.pop();
                    }
                }
            }
            let text = read(&dir);
            let a = text.find("hand line one").expect("hand line one kept");
            let b = text.find("hand line two").expect("hand line two kept");
            assert!(a < b, "seq {seq}: user lines out of order");
            let sum: f64 = live.iter().map(|(_, k)| k).sum();
            let t = daylog::totals(&dir, DAY, 1800).expect("totals");
            assert!((t.macros.kcal - sum).abs() <= 1.0, "seq {seq}: totals {} vs model {sum}", t.macros.kcal);
            assert_eq!(daylog::has_meal(&dir, DAY), !live.is_empty(), "seq {seq}");
            for id in &ever {
                assert!(daylog::logged(&dir, DAY, *id), "seq {seq}: id {id} not logged");
            }
        }
    }
}
