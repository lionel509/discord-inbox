//! Daylog invariants I5–I7 under random hand edits (xorshift64, fixed seed). Std only; scratch dirs
//! live under the OS temp dir and nothing touches a real vault.
use discord_inbox::daylog::{self, Meal, Written};
use discord_inbox::nutrition::Macros;
use std::fs;
use std::path::{Path, PathBuf};

const DAY: &str = "2026-10-26";
const OPEN: &str = "<!-- pip:totals -->";
const CLOSE: &str = "<!-- /pip:totals -->";
const TITLE: &str = "# 2026-10-26";

const FRAGS: &[&str] = &[
    "---\nlast reviewed: 2026-10-20\n---\n",
    "---\n날짜: 2026-10-20\n# my own note\ntags: [food]\n---  \n",
    "# Rough day\n",
    "# 2026-10-26\n",
    "**Total:** 999 kcal\n",
    "**Flags:** none\n",
    "### how I feel\n",
    "### Coffee · msg is fine I guess\n",
    "### 12:31 · msg 5\n",
    "- macros: 500 kcal · P 1 g · C 1 g · F 1 g · veg yes\n",
    "---\n",
    "Felt bloated after lunch, skipped the cabbage.\n",
    "\n",
    "## Meals\n",
    "slept badly\r\n",
    "- [ ] eat something green\n",
    "> 오늘 힘들었어\n",
];

fn rng(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

fn scratch(case: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pip-inv-{}-{case}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
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

/// The interior of the first complete totals pair (STEP 7's rule), as byte offsets `(a, b)`.
fn interior(t: &str) -> Option<(usize, usize)> {
    let (mut pos, mut open) = (0usize, None);
    for line in t.split_inclusive('\n') {
        if line.trim() == OPEN {
            open = Some(pos + line.len());
        } else if line.trim() == CLOSE {
            if let Some(a) = open {
                return Some((a, pos));
            }
        }
        pos += line.len();
    }
    None
}

/// `t` with the totals interior removed.
fn strip(t: &str) -> String {
    match interior(t) {
        Some((a, b)) => format!("{}{}", &t[..a], &t[b..]),
        None => t.to_string(),
    }
}

fn note_text(dir: &Path) -> String {
    fs::read_to_string(daylog::note_path(dir, DAY)).unwrap_or_default()
}

fn meals_of(dir: &Path) -> usize {
    daylog::totals(dir, DAY, 1800).map_or(0, |t| t.meals)
}

fn count_title(t: &str) -> usize {
    t.lines().filter(|l| *l == TITLE).count()
}

/// One write, checked against C1–C4. Returns the id written.
fn write_checked(case: usize, dir: &Path, id: u64) {
    let e = note_text(dir);
    let meals_before = meals_of(dir);
    let r = daylog::write_meal(dir, DAY, &meal(id), 1800).unwrap();
    let o = note_text(dir);
    let ctx = format!("case {case}, write of id {id}\n--- before ---\n{e}\n--- after ---\n{o}");
    assert_eq!(r, Written::Logged, "{ctx}");
    // C1: nothing outside the totals interior moves, and nothing is lost.
    assert!(strip(&o).starts_with(&strip(&e)), "C1 {ctx}");
    // C2: one totals open marker per complete pair; Pip adds one when there was none.
    let opens = |t: &str| t.lines().filter(|l| l.trim() == OPEN).count();
    let expected = opens(&e) + if interior(&e).is_none() { 1 } else { 0 };
    assert_eq!(opens(&o), expected, "C2 {ctx}");
    // C3: a note that existed keeps its title count.
    if !e.is_empty() {
        assert_eq!(count_title(&o), count_title(&e), "C3 {ctx}");
    }
    // C4: the new meal is logged, is a meal, and the count rose by one.
    assert!(daylog::logged(dir, DAY, id), "C4 logged {ctx}");
    assert!(daylog::has_meal(dir, DAY), "C4 has_meal {ctx}");
    assert_eq!(meals_of(dir), meals_before + 1, "C4 meals {ctx}");
}

/// Random line-boundary offsets (every line start, and the end of the text).
fn boundaries(t: &str) -> Vec<usize> {
    let mut v = vec![0];
    let mut pos = 0;
    for line in t.split_inclusive('\n') {
        pos += line.len();
        v.push(pos);
    }
    v
}

/// One hand edit: insert a FRAG at a line boundary, or (1 in 4) delete a whole line. Positions strictly
/// inside the totals interior are skipped, and so is any line that starts inside it.
fn hand_edit(rnd: &mut u64, t: &str) -> String {
    let inside = |p: usize| matches!(interior(t), Some((a, b)) if a <= p && p < b);
    if rng(rnd) % 4 == 0 {
        let lines: Vec<(usize, usize)> = {
            let mut v = Vec::new();
            let mut pos = 0;
            for line in t.split_inclusive('\n') {
                v.push((pos, pos + line.len()));
                pos += line.len();
            }
            v
        };
        let ok: Vec<(usize, usize)> = lines.into_iter().filter(|(s, _)| !inside(*s)).collect();
        if ok.is_empty() {
            return t.to_string();
        }
        let (s, e) = ok[rng(rnd) as usize % ok.len()];
        format!("{}{}", &t[..s], &t[e..])
    } else {
        let ok: Vec<usize> = boundaries(t)
            .into_iter()
            .filter(|p| !matches!(interior(t), Some((a, b)) if a <= *p && *p <= b))
            .collect();
        let p = ok[rng(rnd) as usize % ok.len()];
        let frag = FRAGS[rng(rnd) as usize % FRAGS.len()];
        format!("{}{}{}", &t[..p], frag, &t[p..])
    }
}

/// C5: every id still marked in the note replays as `Already`, leaving the bytes unchanged. An id whose
/// marker he deleted is no longer logged (the plan's RISKS), so it is not replayed.
fn replay_all(case: usize, dir: &Path, ids: &[u64]) {
    for &id in ids {
        if !daylog::logged(dir, DAY, id) {
            continue;
        }
        let before = note_text(dir);
        let r = daylog::write_meal(dir, DAY, &meal(id), 1800).unwrap();
        assert_eq!(r, Written::Already, "C5 case {case}, replay of id {id}\n{before}");
        assert_eq!(note_text(dir), before, "C5 case {case}, replay of id {id} changed the file");
    }
}

#[test]
fn his_bytes_survive_every_write() {
    let mut rnd: u64 = 0x9E3779B97F4A7C15;
    for case in 0..300 {
        let dir = scratch(case);
        let mut next_id = 1000u64;
        let mut ids: Vec<u64> = Vec::new();
        let kind = rng(&mut rnd) % 3;
        let writes = 1 + rng(&mut rnd) as usize % 3;
        match kind {
            0 => {}
            1 => {
                let n = 1 + rng(&mut rnd) as usize % 6;
                let mut text = String::new();
                for _ in 0..n {
                    text.push_str(FRAGS[rng(&mut rnd) as usize % FRAGS.len()]);
                }
                if rng(&mut rnd) % 4 == 0 {
                    text.push_str("no newline at end");
                }
                fs::create_dir_all(&dir).unwrap();
                fs::write(daylog::note_path(&dir, DAY), &text).unwrap();
                assert!(!daylog::has_meal(&dir, DAY), "I7 case {case}\n{text}");
                assert!(!daylog::logged(&dir, DAY, 5), "I7 case {case}\n{text}");
                assert!(!daylog::logged(&dir, DAY, 1000), "I7 case {case}\n{text}");
            }
            _ => {
                let pips = 1 + rng(&mut rnd) as usize % 3;
                for _ in 0..pips {
                    write_checked(case, &dir, next_id);
                    ids.push(next_id);
                    next_id += 1;
                }
                let edits = 1 + rng(&mut rnd) as usize % 3;
                for _ in 0..edits {
                    let t = note_text(&dir);
                    let edited = hand_edit(&mut rnd, &t);
                    fs::write(daylog::note_path(&dir, DAY), &edited).unwrap();
                }
            }
        }
        for _ in 0..writes {
            write_checked(case, &dir, next_id);
            ids.push(next_id);
            next_id += 1;
        }
        replay_all(case, &dir, &ids);
    }
}

#[test]
fn hand_note_is_a_byte_prefix() {
    let dir = scratch(10_000);
    fs::create_dir_all(&dir).unwrap();
    let seed = "---\r\ntags: [food]\r\n---\r\n# Rough day\r\nslept badly";
    fs::write(daylog::note_path(&dir, DAY), seed).unwrap();
    daylog::write_meal(&dir, DAY, &meal(1), 1800).unwrap();
    let out = note_text(&dir);
    assert!(out.starts_with(seed), "{out}");
}

#[test]
fn typed_macros_line_is_not_counted() {
    let dir = scratch(10_001);
    fs::create_dir_all(&dir).unwrap();
    fs::write(daylog::note_path(&dir, DAY), "- macros: 500 kcal · P 1 g · C 1 g · F 1 g · veg yes\n").unwrap();
    let mut m = meal(1);
    m.macros = Macros { kcal: 300.0, protein: 20.0, carbs: 0.0, fat: 14.0 };
    daylog::write_meal(&dir, DAY, &m, 1800).unwrap();
    let t = daylog::totals(&dir, DAY, 1800).unwrap();
    assert_eq!(t.macros.kcal, 300.0);
    assert_eq!(t.meals, 1);
}

#[test]
fn his_correction_inside_a_meal_counts() {
    let dir = scratch(10_002);
    write_checked_plain(&dir, 1, 400.0);
    let t = note_text(&dir);
    let fixed = t.replacen("- macros: 400 kcal", "- macros: 350 kcal", 1);
    assert_ne!(fixed, t, "the meal 1 macros line was not found:\n{t}");
    fs::write(daylog::note_path(&dir, DAY), &fixed).unwrap();
    let mut m2 = meal(2);
    m2.macros = Macros { kcal: 291.0, protein: 10.0, carbs: 30.0, fat: 5.0 };
    daylog::write_meal(&dir, DAY, &m2, 1800).unwrap();
    let t2 = daylog::totals(&dir, DAY, 1800).unwrap();
    assert_eq!(t2.macros.kcal, 641.0);
    let out = note_text(&dir);
    assert_eq!(out.matches("**Total:** 641 kcal").count(), 1, "{out}");
}

#[test]
fn pasted_meal_block_counts_once() {
    let dir = scratch(10_003);
    write_checked_plain(&dir, 1, 400.0);
    let t = note_text(&dir);
    let start = t.find("<!-- pip:meal 1 -->").unwrap();
    let end_marker = "<!-- /pip:meal 1 -->\n";
    let end = t.find(end_marker).unwrap() + end_marker.len();
    let block = t[start..end].to_string();
    fs::write(daylog::note_path(&dir, DAY), format!("{t}{block}")).unwrap();
    daylog::write_meal(&dir, DAY, &meal(2), 1800).unwrap();
    assert_eq!(daylog::totals(&dir, DAY, 1800).unwrap().meals, 2);
}

/// Writes a meal with the given kcal (macros split evenly for protein, carbs and fat).
fn write_checked_plain(dir: &Path, id: u64, kcal: f64) {
    let mut m = meal(id);
    m.macros = Macros { kcal, protein: 20.0, carbs: 0.0, fat: 14.0 };
    assert_eq!(daylog::write_meal(dir, DAY, &m, 1800).unwrap(), Written::Logged);
}
