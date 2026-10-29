//! U3: the gym session wired through `pip::handle` and catch-up replay (fake Discord, as tests/pip_ingest.rs does).
//! The plan text is a made-up fixture, embedded here so this file does not depend on examples/.
use chrono::{DateTime, Duration};
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::gym;
use discord_inbox::ingest::{catch_up_each, History, Inbound, Outcome};
use discord_inbox::pip::{self, Config, PipHandler};
use discord_inbox::checkin;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MAIN: u64 = 100000000000000001;
const MAIN_CH: u64 = 5001;
const DAY: &str = "2026-10-12";
const BASE: &str = "2026-10-12T18:00:00-04:00";

const PLAN: &str = "---
start: 2026-10-12
badminton: Tue Thu
---
# Weeks 1-2
## Day 1
- Dumbbell row
- Reverse lunges or Split squats
- Plank

## Day 2
- Leg press
- Lat pulldown
- Crunch

# Week 3+
## Push
- Bench press
- Overhead press or Dumbbell press
- Triceps pushdown

## Pull
- Cable row
- Face pull

## Legs
- Goblet squat
- Reverse lunges or Split squats

## Any day
- Hanging knee raise

| Dumbbell row | Cable row |
";

#[derive(Clone)]
struct Spec {
    id: u64,
    author: u64,
    received: &'static str,
    text: &'static str,
    files: Vec<(&'static str, Vec<u8>)>,
}

struct FakeMsg {
    spec: Spec,
    live: bool,
    downloads: Arc<AtomicUsize>,
}

impl Inbound for FakeMsg {
    fn author_id(&self) -> u64 {
        self.spec.author
    }
    fn msg_id(&self) -> u64 {
        self.spec.id
    }
    fn channel_id(&self) -> u64 {
        MAIN_CH
    }
    fn received(&self) -> String {
        self.spec.received.to_string()
    }
    fn content(&self) -> String {
        self.spec.text.to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.spec.files.iter().map(|f| f.0.to_string()).collect()
    }
    fn live(&self) -> bool {
        self.live
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        Ok(self.spec.files[i].1.clone())
    }
    async fn react(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    async fn unreact(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        Vec::new()
    }
    async fn reply(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

struct FakeHistory {
    msgs: Mutex<Vec<Spec>>,
}

impl History for FakeHistory {
    type Msg = FakeMsg;
    async fn dm_channel(&self, _user_id: u64) -> Result<u64, String> {
        Ok(MAIN_CH)
    }
    async fn page(&self, channel: u64, after: Option<u64>) -> Result<Vec<FakeMsg>, String> {
        if channel != MAIN_CH {
            return Ok(vec![]);
        }
        Ok(self
            .msgs
            .lock()
            .unwrap()
            .iter()
            .filter(|s| after.is_none_or(|a| s.id > a))
            .map(|s| FakeMsg { spec: s.clone(), live: false, downloads: Arc::new(AtomicUsize::new(0)) })
            .collect())
    }
    async fn fetch(&self, _channel: u64, msg_id: u64) -> Result<FakeMsg, String> {
        self.msgs
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == msg_id)
            .map(|s| FakeMsg { spec: s.clone(), live: false, downloads: Arc::new(AtomicUsize::new(0)) })
            .ok_or_else(|| "gone".to_string())
    }
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-gym-flow-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn cfg(dir: &Path) -> Config {
    Config { nutrition_dir: dir.to_path_buf(), recipes_dir: None, low_kcal: 1800, kcal_target: 2400, model: None, model_timeout: std::time::Duration::from_secs(90) }
}

fn write_plan(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(gym::PLAN_FILE), PLAN).unwrap();
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn at(minutes: i64) -> &'static str {
    let t = DateTime::parse_from_rfc3339(BASE).unwrap() + Duration::minutes(minutes);
    leak(t.to_rfc3339())
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| {
                let p = e.unwrap().path();
                (p.file_name().unwrap().to_string_lossy().into_owned(), fs::read(&p).unwrap_or_default())
            })
            .collect(),
        Err(_) => vec![],
    };
    v.sort();
    // catch-up keeps its own cursor file; the claim and note are what must not change.
    v.retain(|(name, _)| !name.starts_with(".cursor-"));
    v
}

fn note(dir: &Path) -> String {
    fs::read_to_string(daylog::note_path(dir, DAY)).unwrap_or_default()
}

/// A tiny seeded LCG so the random sessions are reproducible; the seed is printed on failure.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[tokio::test]
async fn gym_session_live_then_replayed_twice_logs_once() {
    // (reply text, does it write a block?) — "cable row 12 12" is an off-plan pick, still logged.
    let pool: [(&str, bool); 5] = [("15 15 12", true), ("skip", true), ("cable row 12 12", true), ("felt great", false), ("today", false)];
    for seed in [7u64, 42, 2026] {
        let mut rng = Lcg(seed);
        let dir = scratch(&format!("flow{seed}"));
        write_plan(&dir);
        let a = allow();
        let c = cfg(&dir);
        let mut history = vec![Spec { id: 1, author: MAIN, received: at(0), text: "gym", files: vec![] }];
        let mut expected_blocks = 0usize;
        // The `gym` opener is live too, so the session is open before the first reply.
        let opener = FakeMsg { spec: history[0].clone(), live: true, downloads: Arc::new(AtomicUsize::new(0)) };
        pip::handle(&a, &c, &opener).await;
        let len = 2 + (rng.next() % 5) as usize; // 2..=6 replies
        for i in 0..len {
            // Stop sending once the session has closed (finished or timed out), so no reply lands as a note.
            if gym::read_open(&dir, at(1 + i as i64)).is_none() {
                break;
            }
            let (text, writes) = pool[(rng.next() % pool.len() as u64) as usize];
            let id = 2 + i as u64;
            let spec = Spec { id, author: MAIN, received: at(1 + i as i64), text: leak(text.to_string()), files: vec![] };
            let m = FakeMsg { spec: spec.clone(), live: true, downloads: Arc::new(AtomicUsize::new(0)) };
            pip::handle(&a, &c, &m).await;
            if writes {
                expected_blocks += 1;
            }
            history.push(spec);
        }
        // Close the session if the script ran out before the plan did.
        if gym::read_open(&dir, at(50)).is_some() {
            let id = 2 + history.len() as u64;
            let spec = Spec { id, author: MAIN, received: at(50), text: "stop", files: vec![] };
            let m = FakeMsg { spec: spec.clone(), live: true, downloads: Arc::new(AtomicUsize::new(0)) };
            pip::handle(&a, &c, &m).await;
            history.push(spec);
        }

        let live_note = snapshot(&dir);
        let h = FakeHistory { msgs: Mutex::new(history.clone()) };
        catch_up_each(&a, &dir, &h, &PipHandler { allow: &a, cfg: &c }).await;
        assert_eq!(live_note, snapshot(&dir), "seed {seed}: first replay must add nothing");
        catch_up_each(&a, &dir, &h, &PipHandler { allow: &a, cfg: &c }).await;
        assert_eq!(live_note, snapshot(&dir), "seed {seed}: second replay must add nothing");

        let text = note(&dir);
        assert_eq!(text.matches("<!-- pip:gym ").count(), expected_blocks, "seed {seed}: {text}");
        assert!(!dir.join(".gym-open").exists(), "seed {seed}: no session left open");
        for s in &history {
            assert!(daylog::command_seen(&dir, s.id), "seed {seed}: msg {} must be claimed", s.id);
        }
    }
}

#[tokio::test]
async fn gym_dm_never_counts_as_meal_and_checkin_still_due() {
    let dir = scratch("nomeal");
    write_plan(&dir);
    let a = allow();
    let c = cfg(&dir);
    for (id, text, minutes) in [(1u64, "gym", 0i64), (2, "skip", 1), (3, "stop", 2)] {
        let spec = Spec { id, author: MAIN, received: at(minutes), text: leak(text.to_string()), files: vec![] };
        let m = FakeMsg { spec, live: true, downloads: Arc::new(AtomicUsize::new(0)) };
        pip::handle(&a, &c, &m).await;
    }
    assert!(!daylog::has_meal(&dir, DAY), "a gym DM is not a meal");
    assert_eq!(daylog::totals(&dir, DAY, 1800).map_or(0.0, |t| t.macros.kcal), 0.0);
    let nine = chrono::NaiveDate::from_ymd_opt(2026, 10, 12).unwrap().and_hms_opt(21, 0, 0).unwrap();
    assert_eq!(checkin::due(&dir, nine).as_deref(), Some(DAY), "the 21:00 check-in still fires");
}

#[tokio::test]
async fn unreadable_reply_in_session_writes_no_note() {
    let dir = scratch("unreadable");
    write_plan(&dir);
    let a = allow();
    let c = cfg(&dir);
    let open = FakeMsg { spec: Spec { id: 1, author: MAIN, received: at(0), text: "gym", files: vec![] }, live: true, downloads: Arc::new(AtomicUsize::new(0)) };
    pip::handle(&a, &c, &open).await;
    let bad = FakeMsg { spec: Spec { id: 2, author: MAIN, received: at(1), text: "felt great", files: vec![] }, live: true, downloads: Arc::new(AtomicUsize::new(0)) };
    let out = pip::handle(&a, &c, &bad).await;
    assert!(matches!(out, Outcome::Saved | Outcome::Pending | Outcome::Skipped), "answered, not failed");

    assert!(!note(&dir).contains("pip:note"), "no note block: {}", note(&dir));
    assert!(daylog::owed_before(&dir, DAY, u64::MAX).is_none(), "no owed line left behind");
    assert!(daylog::command_seen(&dir, 2), "the reply is claimed in the ledger");
}

#[test]
fn today_card_has_gym_field_and_footer() {
    let dir = scratch("card");
    write_plan(&dir);
    let card = discord_inbox::summary::day_card(&dir, DAY, 1800, 2400);
    assert!(card.fields.iter().any(|(name, _, _)| name == "Gym"), "{:?}", card.fields);
    assert!(card.footer.contains("gym"), "{}", card.footer);
}
