//! Breaker round 2 against 5eac7fc: three daylog write_meal breaks (fix 1 of round 1).
use discord_inbox::daylog::{self, Meal, Written};
use discord_inbox::nutrition::Macros;
use std::fs;
use std::path::{Path, PathBuf};

const DAY: &str = "2026-10-26";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-break-r2-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn meal(id: u64, time: &str, line: &str, kcal: f64) -> Meal {
    Meal {
        msg_id: id,
        time: time.into(),
        lines: vec![line.into()],
        macros: Macros { kcal, protein: 10.0, carbs: 20.0, fat: 5.0 },
        veg: true,
        ..Default::default()
    }
}

fn read(dir: &Path) -> String {
    fs::read_to_string(dir.join(format!("{DAY}.md"))).unwrap()
}

fn fm_block(extra: &str) -> String {
    format!("---\ndate: {DAY}\n{extra}---\n")
}

const BODY: &str = "## Meals\n\n### 12:00 · msg 1\n- rice\n- macros: 200 kcal · P 4 g · C 44 g · F 1 g · veg yes\n\n";

// Break 1: the user's own front matter keys (tags, aliases) are erased by the next meal write.
#[test]
fn r2_user_front_matter_keys_survive_a_meal_write() {
    let dir = scratch("fm-keys");
    let note = format!("{}# {DAY}\n**Total:** 200 kcal\n**Flags:** none\n\n{BODY}", fm_block("tags: [food]\naliases: [lunch-log]\n"));
    fs::write(dir.join(format!("{DAY}.md")), note).unwrap();
    assert_eq!(daylog::write_meal(&dir, DAY, &meal(2, "18:00", "- eggs", 150.0), 1800).unwrap(), Written::Logged);
    let out = read(&dir);
    assert!(out.contains("tags: [food]"), "user tags line dropped:\n{out}");
    assert!(out.contains("aliases: [lunch-log]"), "user aliases line dropped:\n{out}");
}

// Break 2: a note that opens with a `---` horizontal rule has everything up to the next `---` deleted.
#[test]
fn r2_leading_hr_does_not_erase_text_before_next_rule() {
    let dir = scratch("leading-hr");
    let note = format!("---\nFelt bloated after lunch, skipped the cabbage.\n---\n\n{BODY}");
    fs::write(dir.join(format!("{DAY}.md")), note).unwrap();
    daylog::write_meal(&dir, DAY, &meal(2, "18:00", "- eggs", 150.0), 1800).unwrap();
    let out = read(&dir);
    assert!(out.contains("Felt bloated after lunch, skipped the cabbage."), "free text erased:\n{out}");
}

// Break 3: a blank line between the front matter and the `# ` title leaves the old title in place,
// so every write adds a second `# <day>` title.
#[test]
fn r2_blank_line_after_front_matter_does_not_duplicate_title() {
    let dir = scratch("blank-after-fm");
    let note = format!("{}\n# {DAY}\n\n{BODY}", fm_block("note: x\n"));
    fs::write(dir.join(format!("{DAY}.md")), note).unwrap();
    daylog::write_meal(&dir, DAY, &meal(2, "18:00", "- eggs", 150.0), 1800).unwrap();
    let out = read(&dir);
    let titles = out.lines().filter(|l| *l == format!("# {DAY}")).count();
    let fences = out.lines().filter(|l| *l == "---").count();
    assert_eq!(titles, 1, "title duplicated:\n{out}");
    assert_eq!(fences, 2, "front matter duplicated:\n{out}");
}

