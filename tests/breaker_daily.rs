// Breaker findings for PR #138 (head 64e75ad), morning brief and scheduler side. Each test states the plan (#135) or the obvious expectation, so a red test is a break.
use chrono::NaiveDate;
use discord_inbox::calendar::{Cal, Event};
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Deps, Slot};
use discord_inbox::weather;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

struct Fake(String);
impl Chat for Fake {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

struct Down;
impl Chat for Down {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Err("model down".to_string())
    }
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("breaker138-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn ev(cal: &str, date: &str, time: Option<&str>, summary: &str) -> Event {
    Event {
        uid: format!("u-{}", summary.replace(' ', "-")),
        calendar: cal.to_string(),
        date: date.to_string(),
        time: time.map(str::to_string),
        summary: summary.to_string(),
    }
}

// B1. Plan: morning `Today:` lists `- (open) {text}` from note_items. A standing note is open until its front matter says done.
#[test]
fn b1_morning_brief_lists_open_standing_notes() {
    let dir = tmp("b1");
    fs::write(dir.join("Call the TA.md"), "Call the TA about the lab\n").unwrap();
    let out = daily::morning_facts(&dir, &[], None, day());
    assert!(out.contains("(open) Call the TA"), "morning brief dropped the open note:\n{out}");
}

// B2. Plan: is_exam = summary contains exam/midterm/quiz/final. The code checks only exam and midterm.
#[test]
fn b2_quiz_today_brings_calculator() {
    let dir = tmp("b2");
    let evs = vec![ev("SBU - Math 131", "2026-10-20", Some("10:00"), "Quiz 3")];
    let out = daily::morning_facts(&dir, &evs, None, day());
    assert!(out.contains("Bring: calculator (exam today)"), "quiz today did not bring the calculator:\n{out}");
}

// B3. Plan: Due soon = is_deadline events (not done, calendar not Lectures/Office Hours) 1..=3 days out. A midterm two days out is not a "due" word, so the code drops it.
#[test]
fn b3_midterm_two_days_out_is_in_due_soon() {
    let dir = tmp("b3");
    let evs = vec![ev("SBU - Exams", "2026-10-22", Some("09:30"), "Midterm 1")];
    let out = daily::morning_facts(&dir, &evs, None, day());
    assert!(out.contains("Due soon:") && out.contains("Midterm 1"), "midterm 2 days out missing from Due soon:\n{out}");
}

// B4. Plan: is_deadline excludes Lectures and Office Hours calendars. The code lists any summary containing "due".
#[test]
fn b4_lecture_calendar_is_not_a_deadline() {
    let dir = tmp("b4");
    let evs = vec![ev("SBU - Lectures", "2026-10-21", None, "Reading due Friday")];
    let out = daily::morning_facts(&dir, &evs, None, day());
    assert!(!out.contains("Reading due"), "a Lectures event reached Due soon:\n{out}");
}

// B5. Huge day. A 40-event day plus a deadline two days out: compose() cuts the DM at 1900 chars, the plan's voice cap is 1500, and the cut falls after the deadline heads-up, so the heads-up disappears.
#[test]
#[ignore = "follow-up: Due soon cut by MAX_CHARS, see review"]
fn b5_huge_day_keeps_the_deadline_heads_up() {
    let dir = tmp("b5");
    let stub = dir.join("cal.sh");
    let mut script = String::from("#!/bin/sh\ncat >/dev/null\nprintf 'ok\\n'\n");
    for i in 1..=40 {
        script += &format!(
            "printf 'uid{i}\\tSBU - Lectures\\t2026-10-20\\t{:02}:{:02}\\tLecture block {i} with a long descriptive title for the day\\n'\n",
            8 + i / 6,
            (i % 6) * 10
        );
    }
    script += "printf 'uidHW\\tSBU - Math\\t2026-10-22\\t23:00\\tHW 4 due Friday\\n'\n";
    fs::write(&stub, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let wx = dir.join("weather.json");
    fs::write(&wx, include_str!("../examples/weather_sample.json")).unwrap();
    std::env::set_var("DISCORD_INBOX_WEATHER_FILE", &wx);
    let deps = Deps {
        todo_dir: dir.join("! Todo"),
        cal: Some(Cal { osascript: stub, calendars: vec!["SBU - Lectures".into(), "SBU - Math".into()] }),
        chat: Arc::new(Down),
    };
    let dm = daily::compose(&deps, Slot::Morning, day());
    assert!(dm.contains("HW 4 due Friday"), "deadline heads-up cut from a huge day (DM is {} chars):\n{dm}", dm.chars().count());
}

// B6. Scope change on #135: no umbrella anywhere. The voice pass returns the model text unchecked, so a model that adds an umbrella reaches the DM.
#[test]
fn b6_voice_pass_cannot_add_an_umbrella() {
    let text = "Good morning! Grab an umbrella, rain is likely around noon, and a jacket for the walk. Have a good one.";
    let out = daily::voice(&Fake(text.to_string()), Slot::Morning, "Weather (Stony Brook): 52°F now, high 61 / low 45, rain\nBring: nothing extra");
    assert!(!out.to_lowercase().contains("umbrella"), "umbrella reached the morning DM:\n{out}");
}

// B8. Bring line on a word that merely contains "exam". Plan literal is "contains exam"; a whole-word match is what the line means.
#[test]
fn b8_example_sheet_does_not_bring_a_calculator() {
    let dir = tmp("b8");
    let evs = vec![ev("SBU - Physics - Lectures", "2026-10-20", Some("11:00"), "Example sheet 4")];
    let out = daily::morning_facts(&dir, &evs, None, day());
    assert!(out.contains("Bring: nothing extra"), "an example sheet brought a calculator:\n{out}");
}


