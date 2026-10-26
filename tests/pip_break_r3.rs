//! Breaker round 3 @ 17e8697 (PR #108). Temp dirs only. Each test is a repro; expected = the rule
//! "never lose or duplicate a byte of what he wrote, stay idempotent across N writes".
use discord_inbox::daylog::{self, Meal, Written};
use discord_inbox::nutrition::Macros;
use std::fs;
use std::path::{Path, PathBuf};

const DAY: &str = "2026-10-26";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-break-r3-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn meal(id: u64) -> Meal {
    Meal {
        msg_id: id,
        time: "12:31".into(),
        lines: vec!["100 g salmon".into()],
        macros: Macros { kcal: 208.0, protein: 20.0, carbs: 0.0, fat: 14.0 },
        veg: true,
        ..Default::default()
    }
}

fn seed(dir: &Path, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(daylog::note_path(dir, DAY), text).unwrap();
}

fn read(dir: &Path) -> String {
    fs::read_to_string(daylog::note_path(dir, DAY)).unwrap()
}

fn count(text: &str, needle: &str) -> usize {
    text.lines().filter(|l| *l == needle).count()
}

/// A-series: his front matter key that Pip's parser does not recognise as a key.
/// Expect: one `# 2026-10-26` title and one `**Total:**` line after N writes, his block kept once.
fn check_unshaped_front_matter(name: &str, fm_line: &str, close: &str) -> String {
    let dir = scratch(name);
    seed(&dir, &format!("---\n{fm_line}\n{close}\n\n# {DAY}\n**Total:** 0 kcal · P 0 g · C 0 g · F 0 g · 0 meals · veg no\n**Flags:** \n\n"));
    for i in 1..=10u64 {
        daylog::write_meal(&dir, DAY, &meal(i), 1800).unwrap();
    }
    let t = read(&dir);
    let titles = count(&t, &format!("# {DAY}"));
    let totals = t.lines().filter(|l| l.starts_with("**Total:**")).count();
    let his = t.matches(fm_line).count();
    format!("after 10 writes: titles={titles} totals={totals} his_fm_line_count={his}\n--- note head ---\n{}", t.lines().take(14).collect::<Vec<_>>().join("\n"))
}

#[test]
fn a1_key_with_space_is_still_one_title_after_10_writes() {
    let out = check_unshaped_front_matter("a1", "last reviewed: 2026-10-20", "---");
    assert!(out.starts_with("after 10 writes: titles=1 totals=1 his_fm_line_count=1"), "{out}");
}

#[test]
fn a2_non_ascii_key_is_still_one_title_after_10_writes() {
    let out = check_unshaped_front_matter("a2", "날짜: 2026-10-20", "---");
    assert!(out.starts_with("after 10 writes: titles=1 totals=1 his_fm_line_count=1"), "{out}");
}

#[test]
fn a3_closing_rule_with_trailing_space_is_still_one_title() {
    let out = check_unshaped_front_matter("a3", "tags: [food]", "---  ");
    assert!(out.starts_with("after 10 writes: titles=1 totals=1 his_fm_line_count=1"), "{out}");
}

#[test]
fn a4_yaml_comment_in_front_matter_is_still_one_title() {
    let out = check_unshaped_front_matter("a4", "# my own note about this day", "---");
    assert!(out.starts_with("after 10 writes: titles=1 totals=1 his_fm_line_count=1"), "{out}");
}

/// B: a hand-made heading with the msg marker in prose. Pip's rule: has_meal counts only Pip meal headings.
#[test]
fn b1_typed_heading_with_msg_in_prose_is_not_a_meal() {
    let dir = scratch("b1");
    seed(&dir, "### Coffee · msg is fine I guess\nrough day\n");
    assert!(!daylog::has_meal(&dir, DAY), "has_meal true with no Pip meal");
    assert!(!daylog::logged(&dir, DAY, 0));
}

/// G: realistic path. Pip writes the note, he adds an Obsidian property with a space to the
/// front matter, then Pip writes the next meal. Expect one Pip header and one title.
#[test]
fn g1_pip_note_plus_space_key_added_by_hand_one_write() {
    let dir = scratch("g1");
    daylog::write_meal(&dir, DAY, &meal(1), 1800).unwrap();
    let t = read(&dir);
    let edited = t.replacen("date: 2026-10-26\n", "date: 2026-10-26\nlast reviewed: 2026-10-20\n", 1);
    fs::write(daylog::note_path(&dir, DAY), &edited).unwrap();
    daylog::write_meal(&dir, DAY, &meal(2), 1800).unwrap();
    let t2 = read(&dir);
    assert_eq!(t2.matches(&format!("# {DAY}")).count(), 1, "title duplicated:\n{t2}");
    assert_eq!(t2.matches("date: 2026-10-26").count(), 1, "front matter duplicated:\n{t2}");
    assert_eq!(t2.matches("last reviewed: 2026-10-20").count(), 1, "his key:\n{t2}");
}

/// G2: same edit, then ten more writes. Count must stay at one header.
#[test]
fn g2_space_key_edit_then_ten_writes_stays_one_header() {
    let dir = scratch("g2");
    daylog::write_meal(&dir, DAY, &meal(1), 1800).unwrap();
    let t = read(&dir);
    fs::write(
        daylog::note_path(&dir, DAY),
        t.replacen("date: 2026-10-26\n", "date: 2026-10-26\nlast reviewed: 2026-10-20\n", 1),
    )
    .unwrap();
    for i in 2..=11u64 {
        daylog::write_meal(&dir, DAY, &meal(i), 1800).unwrap();
    }
    let t2 = read(&dir);
    assert_eq!(t2.matches(&format!("# {DAY}")).count(), 1, "{t2}");
    assert_eq!(t2.matches("**Total:**").count(), 1, "{t2}");
    assert_eq!(t2.matches("### ").count(), 11, "{t2}");
}
