//! #159 U3: recipe-note kcal lines (`dishes`), the quoted-number check and the note lookup. Made-up folders under the temp dir.
use discord_inbox::dishes::{self, Dish};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn dish(lines: &[&str]) -> Dish {
    Dish { note: "Garlic Rice".into(), lines: lines.iter().map(|l| l.to_string()).collect() }
}

fn scratch(name: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("di-dishes-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn lines_of_keeps_heading_prose_and_table_kcal() {
    let text = "---\ntitle: Garlic Rice\n---\n# Garlic Rice\n\n## Per serving\n\nAbout 520 kcal a serving.\n\n| Dish | kcal |\n|---|---|\n| Rice | 300 |\n| Egg | 150 |\n\nNo number here.\n";
    let lines = dishes::lines_of(text);
    assert!(lines.iter().any(|l| l.contains("520 kcal")), "{lines:?}");
    assert!(lines.contains(&"Rice: 300 kcal".to_string()), "{lines:?}");
    assert!(lines.contains(&"Egg: 150 kcal".to_string()), "{lines:?}");
    assert!(!lines.iter().any(|l| l.contains("title") || l.contains("No number")), "{lines:?}");
}

#[test]
fn lines_of_caps_at_eight_per_note() {
    let text: String = (0..12).map(|i| format!("Item {i}: {i} kcal\n")).collect();
    assert_eq!(dishes::lines_of(&text).len(), 8);
}

#[test]
fn quoted_single_number() {
    let d = dish(&["About 520 kcal a serving"]);
    assert!(dishes::quoted(&d, 520.0));
    assert!(!dishes::quoted(&d, 521.0));
}

#[test]
fn quoted_thousands_comma() {
    let d = dish(&["Bowl 1,110 kcal total"]);
    assert!(dishes::quoted(&d, 1110.0));
}

#[test]
fn quoted_range_holds_the_number() {
    let d = dish(&["Soup 500–650 kcal"]);
    assert!(dishes::quoted(&d, 575.0));
    assert!(!dishes::quoted(&d, 700.0));
}

#[test]
fn quoted_gap_that_is_not_a_range() {
    let d = dish(&["Sauce 750 ... / 880 kcal"]);
    assert!(dishes::quoted(&d, 880.0));
    assert!(!dishes::quoted(&d, 800.0), "750 ... / 880 is not a range");
}

#[test]
fn quoted_refuses_a_number_not_in_the_note() {
    assert!(!dishes::quoted(&dish(&["About 520 kcal a serving"]), 999.0));
}

#[test]
fn quoted_ignores_dates() {
    let d = dish(&["Soup, made 2026-11-04, 300 kcal"]);
    assert!(!dishes::quoted(&d, 2026.0), "a date is not a kcal");
    assert!(dishes::quoted(&d, 300.0));
}

#[test]
fn load_reads_a_folder_non_recursively_and_skips_links_and_empty_notes() {
    let root = scratch("folder");
    let rec = root.join("Kitchen/Recipes");
    write(&rec.join("Garlic Rice.md"), "---\ntitle: x\n---\n# Garlic Rice\n\nAbout 520 kcal a serving.\n");
    write(&rec.join("Empty.md"), "# Empty\n\nNo numbers.\n");
    write(&rec.join("Sub/Nested.md"), "Nested 300 kcal\n");
    std::os::unix::fs::symlink(rec.join("Garlic Rice.md"), rec.join("Alias.md")).unwrap();
    let got = dishes::load(&[rec]);
    let names: Vec<&str> = got.iter().map(|d| d.note.as_str()).collect();
    assert_eq!(names, vec!["Garlic Rice"], "{got:?}");
    assert!(got[0].lines.iter().any(|l| l.contains("520 kcal")));
}

#[test]
fn load_reads_one_file() {
    let root = scratch("file");
    let f = root.join("Kitchen/Soup.md");
    write(&f, "Soup: 300 kcal a bowl\n");
    let got = dishes::load(&[f]);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].note, "Soup");
}

#[test]
fn load_keeps_every_note_past_forty() {
    // Review round 1 (#161): a silent cap dropped late-alphabet dishes, so every note is loaded.
    let root = scratch("cap");
    for i in 0..45 {
        write(&root.join(format!("Dish {i:02}.md")), "Serving: 300 kcal\n");
    }
    write(&root.join("Zucchini Bake.md"), "Serves 2 · 250 kcal\n");
    let got = dishes::load(&[root]);
    assert_eq!(got.len(), 46);
    assert_eq!(got[0].note, "Dish 00");
    assert!(got.iter().any(|d| d.note == "Zucchini Bake"));
}

#[test]
fn marked_is_the_one_matching_note() {
    let ds = vec![
        Dish { note: "Garlic Rice".into(), lines: vec!["520 kcal".into()] },
        Dish { note: "Egg Fried Rice".into(), lines: vec!["610 kcal".into()] },
    ];
    assert_eq!(dishes::marked("garlic rice", &ds).map(|d| d.note.as_str()), Some("Garlic Rice"));
    assert!(dishes::marked("rice", &ds).is_none());
    let dup = vec![ds[0].clone(), Dish { note: "garlic rice".into(), lines: vec!["500 kcal".into()] }];
    assert!(dishes::marked("Garlic Rice", &dup).is_none(), "several matches is no match");
}

// Review round 1 (#161): a serving count or a weight on the same line is not the kcal; the note list is never cut silently.
#[test]
fn serving_count_is_not_the_kcal() {
    let rice = dish(&["Serves 4 · 320 kcal per serving"]);
    assert!(!dishes::quoted(&rice, 4.0));
    assert!(dishes::quoted(&rice, 320.0));
}

#[test]
fn weight_on_a_bread_line_is_not_the_kcal() {
    let bread = dish(&["Farmhouse Sourdough: 43 g per slice, 115 kcal"]);
    assert!(!dishes::quoted(&bread, 43.0));
    assert!(dishes::quoted(&bread, 115.0));
}

#[test]
fn a_range_on_one_kcal_segment_still_counts() {
    let stew = dish(&["Stew: 320–360 kcal per bowl"]);
    assert!(dishes::quoted(&stew, 340.0));
}
