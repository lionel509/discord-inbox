//! Summaries (U6): the `today` and `week` cards, the streak, and the 22:00 daily and Sunday weekly DMs.
//! Everything is computed from the day notes on disk; the charts carry no text, so the numbers go in the card.
use crate::charts::{self, Bar};
use crate::daylog::{self, MealRow};
use crate::nutrition::Macros;
use crate::rich::Rich;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Weekday};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};

/// Local hour from which the daily and weekly summaries are due.
pub const SUMMARY_HOUR: u32 = 22;
const GREEN: u32 = 0x9ED9A0;
const PINK: u32 = 0xF7B6A3;
const FOOTER: &str = "today · week · undo · fix <meal> · help";

/// Daily targets from the kcal target: protein 20 % of kcal at 4 kcal/g, carbs 50 % at 4, fat 30 % at 9.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Targets {
    pub kcal: f64,
    pub protein: f64,
    pub carbs: f64,
    pub fat: f64,
}

impl Targets {
    pub fn from_kcal(k: u32) -> Targets {
        let k = k as f64;
        Targets { kcal: k, protein: k * 20.0 / 100.0 / 4.0, carbs: k * 50.0 / 100.0 / 4.0, fat: k * 30.0 / 100.0 / 9.0 }
    }
}

/// One day's totals. A day with no live meal has `meals == 0`.
#[derive(Clone, Debug)]
pub struct DayStat {
    pub day: String,
    pub macros: Macros,
    pub meals: usize,
    pub veg: bool,
    pub flags: Vec<&'static str>,
}

pub fn day_stat(dir: &Path, day: &str, low: u32) -> DayStat {
    match daylog::totals(dir, day, low) {
        Some(t) => DayStat { day: day.into(), macros: t.macros, meals: t.meals, veg: t.veg, flags: t.flags },
        None => DayStat { day: day.into(), macros: Macros::default(), meals: 0, veg: false, flags: Vec::new() },
    }
}

/// Seven days ending on `end_day`, oldest first. An unreadable date gives no days.
pub fn week_days(end_day: &str) -> Vec<String> {
    let Ok(end) = NaiveDate::parse_from_str(end_day, "%Y-%m-%d") else { return Vec::new() };
    (0..7).rev().map(|k| (end - Duration::days(k)).format("%Y-%m-%d").to_string()).collect()
}

/// Consecutive days with at least one meal, ending today, or ending yesterday when today has none. Capped at 366.
pub fn streak(dir: &Path, today: &str) -> usize {
    let Ok(mut d) = NaiveDate::parse_from_str(today, "%Y-%m-%d") else { return 0 };
    if !daylog::has_meal(dir, today) {
        d -= Duration::days(1);
    }
    let mut n = 0;
    while n < 366 && daylog::has_meal(dir, &d.format("%Y-%m-%d").to_string()) {
        n += 1;
        d -= Duration::days(1);
    }
    n
}

/// `1800`-style whole numbers without a decimal, otherwise one decimal place.
fn num(x: f64) -> String {
    if (x - x.round()).abs() < 0.05 {
        format!("{:.0}", x.round())
    } else {
        format!("{x:.1}")
    }
}

/// One meal as `time · kcal · items`: each item cut at its ` · ` and its ` (`, at most four shown.
fn meal_line(r: &MealRow) -> String {
    let items: Vec<String> = r
        .lines
        .iter()
        .map(|l| {
            let head = l.split(" · ").next().unwrap_or(l);
            head.split(" (").next().unwrap_or(head).trim().to_string()
        })
        .collect();
    let shown = if items.len() > 4 { format!("{}, …", items[..4].join(", ")) } else { items.join(", ") };
    format!("{} · {} kcal · {shown}", r.time, r.kcal.round() as i64)
}

/// The Meals field value: meal lines in order while the joined text stays within 1000 characters (Discord caps a
/// value at 1024). Rows that do not fit are counted in a final `+N more` line, so none vanish silently.
fn meals_value(meals: &[String]) -> String {
    if meals.is_empty() {
        return "none logged".to_string();
    }
    let mut out = String::new();
    let mut shown = 0;
    for m in meals {
        let next = if shown == 0 { m.clone() } else { format!("{out}\n{m}") };
        if next.chars().count() > 1000 {
            break;
        }
        out = next;
        shown += 1;
    }
    if shown == 0 {
        out = meals[0].chars().take(1000).collect();
    }
    let left = meals.len() - shown.max(1);
    if left > 0 {
        out += &format!("\n+{left} more");
    }
    out
}

/// Field rows as `Name: value` lines, with the title first: the plain-text fallback of a card.
fn plain(title: &str, fields: &[(String, String, bool)]) -> String {
    std::iter::once(title.to_string())
        .chain(fields.iter().map(|(n, v, _)| format!("{n}: {v}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `today` card: kcal against target, the three macros against their targets, veg, each meal, flags, streak, and the day chart.
pub fn day_card(dir: &Path, day: &str, low: u32, kcal_target: u32) -> Rich {
    let t = Targets::from_kcal(kcal_target);
    let s = day_stat(dir, day, low);
    let kcal = s.macros.kcal.round() as i64;
    let meals: Vec<String> = daylog::meals(dir, day).iter().map(meal_line).collect();
    let flags = if s.flags.is_empty() {
        "none".to_string()
    } else {
        s.flags.iter().map(|f| f.replace('-', " ")).collect::<Vec<_>>().join(", ")
    };
    let veg = if s.veg { "yes" } else { "no" };
    let fields = vec![
        ("Protein".to_string(), format!("{} / {} g", num(s.macros.protein), num(t.protein)), true),
        ("Carbs".to_string(), format!("{} / {} g", num(s.macros.carbs), num(t.carbs)), true),
        ("Fat".to_string(), format!("{} / {} g", num(s.macros.fat), num(t.fat)), true),
        ("Veg".to_string(), veg.to_string(), true),
        ("Meals".to_string(), meals_value(&meals), false),
        ("Flags".to_string(), flags, true),
        ("Streak".to_string(), format!("{} day(s)", streak(dir, day)), true),
    ];
    let title = format!("{day} · {kcal} / {kcal_target} kcal");
    let image = charts::day_png(
        [s.macros.kcal, s.macros.protein, s.macros.carbs, s.macros.fat],
        [t.kcal, t.protein, t.carbs, t.fat],
    );
    Rich {
        text: plain(&title, &fields),
        title,
        description: String::new(),
        fields,
        color: if s.veg { GREEN } else { PINK },
        footer: FOOTER.into(),
        image: Some(("day.png".into(), image)),
    }
}

/// `Mon 10-26`, or the date as given when it does not parse.
fn day_label(day: &str) -> String {
    match NaiveDate::parse_from_str(day, "%Y-%m-%d") {
        Ok(d) => format!("{} {}", d.format("%a"), &day[5..]),
        Err(_) => day.to_string(),
    }
}

/// The `week` card: the seven days as numbers, the averages and counts, and the week chart (no text in the chart).
pub fn week_card(dir: &Path, end_day: &str, low: u32, kcal_target: u32) -> Rich {
    let t = Targets::from_kcal(kcal_target);
    let days = week_days(end_day);
    let stats: Vec<DayStat> = days.iter().map(|d| day_stat(dir, d, low)).collect();
    let logged: Vec<&DayStat> = stats.iter().filter(|s| s.meals > 0).collect();
    let avg = if logged.is_empty() { 0.0 } else { logged.iter().map(|s| s.macros.kcal).sum::<f64>() / logged.len() as f64 };
    let at_target = logged.iter().filter(|s| s.macros.kcal >= t.kcal).count();
    let veg_days = logged.iter().filter(|s| s.veg).count();
    let first = days.first().cloned().unwrap_or_else(|| end_day.to_string());
    let title = format!("Week {first} → {end_day}");
    let description = format!(
        "Average {} kcal over {} logged day(s) against a {} kcal target · at target {at_target}/7 · veg {veg_days}/7 · streak {} day(s)\nBars: oldest → newest (left to right)",
        avg.round() as i64,
        logged.len(),
        kcal_target,
        streak(dir, end_day)
    );
    let lines: Vec<String> = stats
        .iter()
        .map(|s| {
            let label = day_label(&s.day);
            if s.meals == 0 {
                format!("{label} · not logged")
            } else {
                format!("{label} · {} kcal · veg {}", s.macros.kcal.round() as i64, if s.veg { "✓" } else { "✗" })
            }
        })
        .collect();
    let fields = vec![("Days".to_string(), lines.join("\n"), false)];
    let bars: Vec<Bar> = stats.iter().map(|s| Bar { kcal: s.macros.kcal, logged: s.meals > 0, veg: s.veg }).collect();
    let split = stats.iter().fold([0.0f64; 3], |mut a, s| {
        a[0] += s.macros.protein * 4.0;
        a[1] += s.macros.carbs * 4.0;
        a[2] += s.macros.fat * 9.0;
        a
    });
    let image = charts::week_png(&bars, t.kcal, low as f64, split);
    let text = format!("{}\n{description}\n{}", title, fields[0].1);
    Rich {
        text,
        title,
        description,
        fields,
        color: GREEN,
        footer: FOOTER.into(),
        image: Some(("week.png".into(), image)),
    }
}

/// The daily summary's marker (`.summary-DAY`) in the nutrition folder.
pub fn marker_path(dir: &Path, day: &str) -> PathBuf {
    dir.join(format!(".summary-{day}"))
}

/// The weekly summary's marker (`.weekly-DAY`, keyed by its Sunday).
pub fn weekly_marker(dir: &Path, day: &str) -> PathBuf {
    dir.join(format!(".weekly-{day}"))
}

/// The day whose daily summary is due: from 22:00, once, and only when that day has a meal.
pub fn daily_due(dir: &Path, now: NaiveDateTime) -> Option<String> {
    if now.hour() < SUMMARY_HOUR {
        return None;
    }
    let day = now.date().format("%Y-%m-%d").to_string();
    if marker_path(dir, &day).exists() || !daylog::has_meal(dir, &day) {
        return None;
    }
    Some(day)
}

/// The Sunday whose weekly summary is due: from 22:00 on a Sunday, once, and only when some day of that week has a
/// meal.
pub fn weekly_due(dir: &Path, now: NaiveDateTime) -> Option<String> {
    if now.weekday() != Weekday::Sun || now.hour() < SUMMARY_HOUR {
        return None;
    }
    let day = now.date().format("%Y-%m-%d").to_string();
    if weekly_marker(dir, &day).exists() || !week_days(&day).iter().any(|d| daylog::has_meal(dir, d)) {
        return None;
    }
    Some(day)
}

/// Writes the marker before a send, so a slow send is not sent twice.
pub fn claim(marker: &Path) -> Result<(), String> {
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(marker, "sent\n").map_err(|e| e.to_string())
}

/// Removes a marker after a failed send, so the day can retry.
pub fn release(marker: &Path) {
    let _ = fs::remove_file(marker);
}

/// Claims the marker, runs the send, and releases the marker when the send fails.
pub fn send_marked<F>(marker: &Path, f: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    claim(marker)?;
    f().inspect_err(|_| release(marker))
}

/// One recipient's marker: the day's marker with `-{uid}` appended to its name.
pub fn recipient_marker(marker: &Path, uid: u64) -> PathBuf {
    let name = marker.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    marker.with_file_name(format!("{name}-{uid}"))
}

/// One recipient's staged marker, written before its send and renamed to the recipient's marker after it.
fn staged_marker(mine: &Path) -> PathBuf {
    let name = mine.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    mine.with_file_name(format!("{name}.tmp"))
}

/// Sends to each recipient in order, at least once per day (I3). A recipient whose marker exists is skipped. Before
/// each send, the recipient's marker is staged (`{marker}.tmp`); if the staging write fails, that recipient is not
/// sent and the error is collected. After a successful send the staged file is renamed to the recipient's marker. A
/// failed send removes the staged file, so the recipient is retried. A repeat therefore happens only when a DM was
/// sent and its marker could not be finalized (a crash, or a rename that fails mid-pass), and then once more. The day
/// marker is claimed only when every recipient has been sent, so a retry reaches just the ones that failed.
pub async fn send_each<F, Fut>(marker: &Path, ids: &[u64], mut send: F) -> Result<(), String>
where
    F: FnMut(u64) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let mut errors = Vec::new();
    for &uid in ids {
        let mine = recipient_marker(marker, uid);
        if mine.exists() {
            continue;
        }
        let staged = staged_marker(&mine);
        if let Err(e) = claim(&staged) {
            errors.push(e);
            continue;
        }
        match send(uid).await {
            Ok(()) => {
                if let Err(e) = fs::rename(&staged, &mine).map_err(|e| e.to_string()) {
                    errors.push(e);
                }
            }
            Err(e) => {
                release(&staged);
                errors.push(e);
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    claim(marker)
}
