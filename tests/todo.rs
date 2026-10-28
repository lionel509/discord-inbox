// Daily brief todo files (#135): dated day files, note files, strikes, carry-over and the check-in log. Fixtures live in temp dirs; no vault is touched.
use chrono::{Days, NaiveDate};
use discord_inbox::todo::{self, Item, Place};
use std::fs;
use std::path::{Path, PathBuf};

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("di-todo-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn texts(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.text.clone()).collect()
}

fn fixture(name: &str) -> PathBuf {
    let dir = tmp(name);
    // Line indices: 0 '---', 1 'tags:', 2 '  - todo', 3 '---', 4 Hub, 5 open, 6 struck, 7 bullet, 8 open.
    fs::write(dir.join("2026-10-20.md"), "---\ntags:\n  - todo\n---\nHub: [[Home]]\n- email the TA\n~~struck thing~~\n- buy calculator\nread chapter 4\n").unwrap();
    fs::write(dir.join("Idea.md"), "---\ntags:\n  - idea\n---\nsketch the stand\n").unwrap();
    fs::write(dir.join("Lab note.md"), "lab results\n").unwrap();
    fs::write(dir.join("no-front-matter.md"), "just text\n").unwrap();
    fs::write(dir.join("Old note.md"), "---\ndone: 2026-10-19\n---\nold\n").unwrap();
    fs::write(dir.join(todo::CHECKIN_LOG), "---\ntags:\n  - todo\n---\n").unwrap();
    fs::write(dir.join("scratch.txt"), "not a note\n").unwrap();
    dir
}

#[test]
fn day_items_skip_headers_and_struck_lines() {
    let dir = fixture("day");
    let items = todo::day_items(&dir, d(20));
    assert_eq!(texts(&items), vec!["email the TA", "buy calculator", "read chapter 4"]);
    assert_eq!(items[0].place, Place::Line { file: todo::day_file(&dir, d(20)), idx: 5 });
    assert_eq!(items[2].place, Place::Line { file: todo::day_file(&dir, d(20)), idx: 8 });
}

#[test]
fn day_items_of_a_missing_file_is_empty() {
    let dir = fixture("missing");
    assert!(todo::day_items(&dir, d(25)).is_empty());
}

#[test]
fn note_items_are_open_notes_only() {
    let dir = fixture("notes");
    let notes = todo::note_items(&dir);
    assert_eq!(texts(&notes), vec!["Idea", "Lab note", "no-front-matter"]);
    assert_eq!(notes[1].place, Place::Note { file: dir.join("Lab note.md") });
}

#[test]
fn mark_done_strikes_only_that_line() {
    let dir = fixture("strike");
    let before = fs::read_to_string(todo::day_file(&dir, d(20))).unwrap();
    let item = todo::day_items(&dir, d(20))[0].clone();
    todo::mark_done(&item, d(20)).unwrap();
    let after = fs::read_to_string(todo::day_file(&dir, d(20))).unwrap();
    assert_eq!(after.lines().count(), before.lines().count());
    assert_eq!(after.lines().nth(5), Some("~~email the TA~~"));
    assert_eq!(texts(&todo::day_items(&dir, d(20))), vec!["buy calculator", "read chapter 4"]);
}

#[test]
fn mark_done_closes_notes_with_and_without_front_matter() {
    let dir = fixture("close");
    let lab = todo::note_items(&dir).into_iter().find(|i| i.text == "Lab note").unwrap();
    todo::mark_done(&lab, d(20)).unwrap();
    let body = fs::read_to_string(dir.join("Lab note.md")).unwrap();
    assert!(body.starts_with("---\ndone: 2026-10-20\n---\n"), "{body}");

    let idea = todo::note_items(&dir).into_iter().find(|i| i.text == "Idea").unwrap();
    todo::mark_done(&idea, d(20)).unwrap();
    let body = fs::read_to_string(dir.join("Idea.md")).unwrap();
    assert!(body.contains("done: 2026-10-20"), "{body}");
    assert!(body.contains("sketch the stand"), "{body}");

    let left = texts(&todo::note_items(&dir));
    assert_eq!(left, vec!["no-front-matter"]);
}

#[test]
fn carry_creates_tomorrow_and_skips_duplicates() {
    let dir = fixture("carry");
    let n = todo::carry(&dir, d(21), &["email the TA".to_string(), "read chapter 4".to_string()]).unwrap();
    assert_eq!(n, 2);
    let body = fs::read_to_string(todo::day_file(&dir, d(21))).unwrap();
    assert!(body.starts_with("---\ntags:\n  - todo\n---\n"), "{body}");
    assert_eq!(texts(&todo::day_items(&dir, d(21))), vec!["email the TA", "read chapter 4"]);

    let again = todo::carry(&dir, d(21), &["read chapter 4".to_string(), "new one".to_string()]).unwrap();
    assert_eq!(again, 1);
    assert_eq!(texts(&todo::day_items(&dir, d(21))), vec!["email the TA", "read chapter 4", "new one"]);
}

#[test]
fn log_checkin_appends_every_night() {
    let dir = fixture("log");
    let done = vec!["email the TA".to_string()];
    let carried = vec!["read chapter 4".to_string()];
    todo::log_checkin(&dir, d(20), "Good day", &done, &carried).unwrap();
    todo::log_checkin(&dir, d(20), "Good day", &done, &carried).unwrap();
    let log = fs::read_to_string(dir.join(todo::CHECKIN_LOG)).unwrap();
    assert_eq!(log.matches("## 2026-10-20").count(), 2, "{log}");
    assert!(log.contains("Done: email the TA"), "{log}");
    assert!(log.contains("Carried: read chapter 4"), "{log}");
}

#[test]
fn mark_done_changes_only_the_one_line_on_random_files() {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let dir = tmp("invariant");
    let pool = ["- open item", "read ch", "~~struck~~", "- bullet", "Hub: [[Home]]", "# heading", "", "  plain"];
    let base = d(1);
    for i in 0..50u64 {
        let day = base.checked_add_days(Days::new(i)).unwrap();
        let n = next(13) as usize;
        let lines: Vec<String> = (0..n).map(|k| format!("{} {k}", pool[next(pool.len() as u64) as usize])).collect();
        fs::write(todo::day_file(&dir, day), lines.join("\n") + "\n").unwrap();
        let items = todo::day_items(&dir, day);
        if items.is_empty() {
            continue;
        }
        let pick = items[next(items.len() as u64) as usize].clone();
        let before = fs::read_to_string(todo::day_file(&dir, day)).unwrap();
        todo::mark_done(&pick, day).unwrap();
        let after = fs::read_to_string(todo::day_file(&dir, day)).unwrap();
        let (b, a): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
        assert_eq!(a.len(), b.len(), "{day}: line count changed");
        let idx = match pick.place {
            Place::Line { idx, .. } => idx,
            Place::Note { .. } => panic!("a day file item is a line"),
        };
        for (j, (x, y)) in b.iter().zip(a.iter()).enumerate() {
            if j != idx {
                assert_eq!(x, y, "{day}: line {j} changed");
            }
        }
        assert_eq!(todo::day_items(&dir, day).len(), items.len() - 1, "{day}");
    }
}
