//! Breaker round 4 against b538f0d: marker damage probes for daylog write_meal, logged, has_meal, totals.
use discord_inbox::daylog::{self, Meal, Written};
use discord_inbox::nutrition::Macros;
use std::fs;
use std::path::{Path, PathBuf};

const DAY: &str = "2026-10-26";
const LOW: u32 = 1800;
const OPEN: &str = "<!-- pip:totals -->";
const CLOSE: &str = "<!-- /pip:totals -->";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-break-r4-{}-{name}", std::process::id()));
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

fn put(dir: &Path, t: &str) {
    fs::write(file(dir), t).unwrap();
}

/// Removes the nth (0-based) line whose trimmed text equals `needle`.
fn drop_nth(t: &str, needle: &str, n: usize) -> String {
    let mut seen = 0;
    let mut out = String::new();
    for line in t.split_inclusive('\n') {
        if line.trim() == needle {
            let k = seen;
            seen += 1;
            if k == n {
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

/// Inserts `ins` right after the first line whose trimmed text equals `needle`.
fn insert_after(t: &str, needle: &str, ins: &str) -> String {
    let mut out = String::new();
    let mut done = false;
    for line in t.split_inclusive('\n') {
        out.push_str(line);
        if !done && line.trim() == needle {
            out.push_str(ins);
            done = true;
        }
    }
    out
}

fn count(t: &str, needle: &str) -> usize {
    t.lines().filter(|l| l.trim() == needle).count()
}

fn kcal(dir: &Path) -> i64 {
    daylog::totals(dir, DAY, LOW).map(|t| t.macros.kcal.round() as i64).unwrap_or(-1)
}

// P01: the residual hazard from the coder's note. Close of pair 1 deleted, Pip adds pair 2 before meal 2,
// then the open of pair 2 is deleted. Pair 1's open now pairs with pair 2's close and spans meal 1.
#[test]
fn p01_two_marker_deletions_must_not_erase_a_meal() {
    let dir = scratch("p01");
    assert_eq!(w(&dir, 1, 400.0), Written::Logged);
    put(&dir, &drop_nth(&read(&dir), CLOSE, 0));
    assert_eq!(w(&dir, 2, 291.0), Written::Logged);
    put(&dir, &drop_nth(&read(&dir), OPEN, 1));
    assert_eq!(w(&dir, 3, 150.0), Written::Logged);
    let t = read(&dir);
    assert!(daylog::logged(&dir, DAY, 1), "meal 1 erased:\n{t}");
    assert!(daylog::logged(&dir, DAY, 2), "meal 2 erased:\n{t}");
    assert_eq!(daylog::totals(&dir, DAY, LOW).unwrap().meals, 3, "{t}");
}

// P02: a line he types between the totals markers. Obsidian's reading view hides the markers.
#[test]
fn p02_his_line_typed_inside_totals_markers_survives() {
    let dir = scratch("p02");
    w(&dir, 1, 400.0);
    put(&dir, &insert_after(&read(&dir), OPEN, "Felt bloated after lunch, skipped the cabbage.\n"));
    w(&dir, 2, 291.0);
    let t = read(&dir);
    assert!(t.contains("Felt bloated after lunch"), "his line erased:\n{t}");
}

// P04: he deletes a meal's open marker. A retry of the same message (bot restart) must change nothing.
#[test]
fn p04_deleted_open_marker_then_relog_must_be_already() {
    let dir = scratch("p04");
    w(&dir, 1, 400.0);
    put(&dir, &drop_nth(&read(&dir), "<!-- pip:meal 1 -->", 0));
    let r = w(&dir, 1, 400.0);
    let t = read(&dir);
    assert_eq!(count(&t, "### 12:31 · msg 1"), 1, "relog duplicated the meal:\n{t}");
    assert_eq!(r, Written::Already);
}

// P07: he deletes the totals close marker. Pip must not leave two totals blocks behind.
#[test]
fn p07_deleted_totals_close_leaves_one_totals_block() {
    let dir = scratch("p07");
    w(&dir, 1, 400.0);
    put(&dir, &drop_nth(&read(&dir), CLOSE, 0));
    w(&dir, 2, 291.0);
    let t = read(&dir);
    let n = t.lines().filter(|l| l.starts_with("**Total:**")).count();
    assert_eq!(n, 1, "two **Total:** lines:\n{t}");
}

// P08: seeded marker-damage fuzz. Typed sentinel lines sit outside the totals markers. Each round applies
// 1-2 random marker edits (delete, duplicate, swap with neighbour), then writes one meal. Every sentinel
// must still appear exactly once, and no marker damage may leave a meal's own id logged twice.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn is_marker(l: &str) -> bool {
    let t = l.trim();
    t.starts_with("<!-- ") && t.contains("pip:")
}

fn damage(lines: &mut Vec<String>, rng: &mut Rng) -> String {
    let mut desc = String::new();
    for _ in 0..(1 + rng.below(2)) {
        let markers: Vec<usize> = (0..lines.len()).filter(|&i| is_marker(&lines[i])).collect();
        if markers.is_empty() {
            break;
        }
        let m = markers[rng.below(markers.len())];
        match rng.below(3) {
            0 => {
                desc += &format!("delete[{}] ", lines[m].trim());
                lines.remove(m);
            }
            1 => {
                desc += &format!("dup[{}] ", lines[m].trim());
                let l = lines[m].clone();
                lines.insert(m, l);
            }
            _ => {
                desc += &format!("swap[{}] ", lines[m].trim());
                if m + 1 < lines.len() {
                    lines.swap(m, m + 1);
                }
            }
        }
    }
    desc
}

fn insert_sentinel(lines: &mut Vec<String>, s: &str, rng: &mut Rng) {
    // Outside the single totals pair if one exists; any boundary otherwise.
    let open = lines.iter().position(|l| l.trim() == OPEN);
    let close = lines.iter().position(|l| l.trim() == CLOSE);
    let ok: Vec<usize> = (0..=lines.len())
        .filter(|&i| match (open, close) {
            (Some(a), Some(b)) => !(a < i && i <= b),
            _ => true,
        })
        .collect();
    let at = ok[rng.below(ok.len())];
    lines.insert(at, format!("{s}\n"));
}

fn fuzz_case(seed: u64) -> Result<(), String> {
    let dir = scratch(&format!("fz{seed}"));
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    for id in 1..=3 {
        w(&dir, id, 200.0 + id as f64);
    }
    let mut lines: Vec<String> = read(&dir).split_inclusive('\n').map(|s| s.to_string()).collect();
    for k in 0..3 {
        insert_sentinel(&mut lines, &format!("SENT-{k}-{seed}"), &mut rng);
    }
    put(&dir, &lines.concat());
    let mut log = String::new();
    for round in 0..4u64 {
        let mut ls: Vec<String> = read(&dir).split_inclusive('\n').map(|s| s.to_string()).collect();
        let d = damage(&mut ls, &mut rng);
        put(&dir, &ls.concat());
        log += &format!("r{round}: {d}| ");
        w(&dir, 10 + round, 150.0);
    }
    let out = read(&dir);
    for k in 0..3 {
        let s = format!("SENT-{k}-{seed}");
        let c = out.lines().filter(|l| l.trim() == s).count();
        if c != 1 {
            return Err(format!("{log}sentinel {s} found {c} times\n{out}"));
        }
    }
    Ok(())
}

#[test]
fn p08_marker_damage_fuzz_never_loses_or_duplicates_his_lines() {
    let mut fails = Vec::new();
    for seed in 1..=300u64 {
        if let Err(e) = fuzz_case(seed) {
            fails.push(format!("seed {seed}: {e}"));
        }
    }
    assert!(fails.is_empty(), "{} of 300 failed; first:\n{}", fails.len(), fails.first().cloned().unwrap_or_default());
}

// P06b: same duplicated open marker, then one more meal. The stored totals must agree with the real meals.
#[test]
fn p06b_duplicated_open_marker_stored_totals_after_next_write() {
    let dir = scratch("p06b");
    w(&dir, 1, 400.0);
    let open = "<!-- pip:meal 1 -->\n";
    put(&dir, &read(&dir).replacen(open, &format!("{open}{open}"), 1));
    w(&dir, 2, 291.0);
    assert_eq!(kcal(&dir), 691, "live totals:\n{}", read(&dir));
    let stored = read(&dir).lines().find(|l| l.starts_with("**Total:**")).unwrap_or("").to_string();
    assert!(stored.contains("691 kcal"), "stored totals: {stored}");
}
