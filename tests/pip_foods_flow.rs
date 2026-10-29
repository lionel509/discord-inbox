//! #146: a label photo saves a food note; a later count logs from it. Through `pip::handle` with the reader stubbed by a
//! `/bin/sh` helper (`PIP_MODEL`), as in `tests/pip_model_flow.rs`. Scratch dirs under the temp dir; no live model.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAIN: u64 = 100000000000000001;
const MAIN_CH: u64 = 5001;
const LABEL: &str = r#"{"label":{"name":"Northwind Pork Wontons","serving_g":140,"serving_pc":8,"kcal":230,"protein":9,"carbs":28,"fat":9},"items":[],"unreadable":[]}"#;

struct Msg {
    id: u64,
    received: String,
    text: String,
    files: Vec<String>,
    replies: Arc<Mutex<Vec<String>>>,
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        MAIN
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        MAIN_CH
    }
    fn received(&self) -> String {
        self.received.clone()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.files.clone()
    }
    fn live(&self) -> bool {
        false
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(b"jpeg bytes".to_vec())
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
    async fn reply(&self, text: &str) -> Result<(), String> {
        self.replies.lock().unwrap().push(text.to_string());
        Ok(())
    }
    async fn reply_rich(&self, r: &discord_inbox::rich::Rich) -> Result<(), String> {
        self.replies.lock().unwrap().push(format!("{:?}", r));
        Ok(())
    }
}

/// Nutrition dir, foods dir, and the root (helper, its output and its call log live there).
fn world(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pip-foods-flow-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Nutrition");
    let foods = root.join("Private").join("Pantry").join("Foods");
    fs::create_dir_all(&dir).unwrap();
    (dir, foods, root)
}

fn set_reply(root: &Path, json: &str) {
    fs::write(root.join("model-out"), json).unwrap();
}

fn reply_json(items: &str) -> String {
    format!("{{\"items\":[{items}],\"unreadable\":[]}}")
}

fn item(name: &str, grams: f64, kcal: f64) -> String {
    format!(r#"{{"name":"{name}","grams":{grams},"typed":false,"kcal_100":{kcal},"protein_100":10,"carbs_100":10,"fat_100":5,"veg":false}}"#)
}

/// A `/bin/sh` reader: appends its arg count to a call log, then prints the reply file.
fn stub(root: &Path) -> PathBuf {
    let p = root.join("helper.sh");
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{args}'\ncat >> '{stdin}'\ncat '{out}'\n",
        args = root.join("model-args").display(),
        stdin = root.join("model-stdin").display(),
        out = root.join("model-out").display(),
    );
    fs::write(&p, body).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn prompts(root: &Path) -> String {
    fs::read_to_string(root.join("model-stdin")).unwrap_or_default()
}

fn calls(root: &Path) -> usize {
    fs::read_to_string(root.join("model-args")).unwrap_or_default().lines().count()
}

fn cfg(dir: &Path, foods: Option<&Path>, model: Option<PathBuf>) -> Config {
    Config {
        nutrition_dir: dir.to_path_buf(),
        recipes_dir: None,
        foods_dir: foods.map(Path::to_path_buf),
        low_kcal: 1800,
        kcal_target: 2400,
        model,
        model_timeout: Duration::from_secs(5),
    }
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn msg(id: u64, text: &str, files: &[&str]) -> (Msg, Arc<Mutex<Vec<String>>>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg {
        id,
        received: "2026-10-29T12:00:00-04:00".to_string(),
        text: text.to_string(),
        files: files.iter().map(|f| f.to_string()).collect(),
        replies: replies.clone(),
    };
    (m, replies)
}

async fn run(c: &Config, m: &Msg) -> Outcome {
    pip::handle(&allow(), c, m).await
}

fn note(dir: &Path) -> String {
    fs::read_to_string(daylog::note_path(dir, "2026-10-29")).unwrap_or_default()
}

fn joined(r: &Arc<Mutex<Vec<String>>>) -> String {
    r.lock().unwrap().join("\n")
}

fn md_count(dir: &Path) -> usize {
    fs::read_dir(dir).map(|rd| rd.filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "md")).count()).unwrap_or(0)
}

#[tokio::test]
async fn a_label_photo_saves_a_food_and_no_meal() {
    let (dir, foods, root) = world("save");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    let (m, replies) = msg(4001, "", &["label.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(foods.join("Northwind Pork Wontons.md").exists());
    assert!(foods.join("Foods.md").exists());
    assert!(!note(&dir).contains("<!-- pip:meal 4001 -->"), "{}", note(&dir));
    assert!(joined(&replies).contains("Saved Northwind Pork Wontons"), "{}", joined(&replies));
    assert_eq!(calls(&root), 1);
}

#[tokio::test]
async fn a_count_logs_from_the_saved_label_without_the_reader() {
    let (dir, foods, root) = world("count");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    assert_eq!(run(&c, &msg(4001, "", &["label.jpg"]).0).await, Outcome::Saved);
    let (m, _) = msg(4002, "24 wontons", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert_eq!(calls(&root), 1, "a typed count from a saved food needs no reader call");
    let n = note(&dir);
    assert!(n.contains("<!-- pip:meal 4002 -->"), "{n}");
    assert!(n.contains("(24 pc, label)"), "{n}");
    assert!(n.contains("690 kcal"), "{n}");
    let food = fs::read_to_string(foods.join("Northwind Pork Wontons.md")).unwrap();
    assert!(food.contains("- last: 24 pc"), "{food}");
}

#[tokio::test]
async fn a_bare_name_uses_the_last_count_as_the_default() {
    let (dir, foods, root) = world("default");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    run(&c, &msg(4001, "", &["label.jpg"]).0).await;
    run(&c, &msg(4002, "24 wontons", &[]).0).await;
    run(&c, &msg(4003, "wontons", &[]).0).await;
    assert!(note(&dir).contains("(default 24 pc, label)"), "{}", note(&dir));
}

#[tokio::test]
async fn a_replayed_label_message_saves_nothing_twice() {
    let (dir, foods, root) = world("replay");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    let (m, _) = msg(4001, "", &["label.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let before = calls(&root);
    assert_eq!(run(&c, &m).await, Outcome::Skipped);
    assert_eq!(calls(&root), before);
    assert_eq!(md_count(&foods), 2, "one food note and the index");
}

#[tokio::test]
async fn a_second_label_photo_updates_the_same_note() {
    let (dir, foods, root) = world("update");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    run(&c, &msg(4001, "", &["label.jpg"]).0).await;
    set_reply(&root, &LABEL.replace("\"kcal\":230", "\"kcal\":250"));
    assert_eq!(run(&c, &msg(4005, "", &["label.jpg"]).0).await, Outcome::Saved);
    assert_eq!(md_count(&foods), 2);
    let food = fs::read_to_string(foods.join("Northwind Pork Wontons.md")).unwrap();
    assert!(food.contains("- per serving: 250 kcal"), "{food}");
}

#[tokio::test]
async fn a_meal_photo_never_writes_a_food() {
    let (dir, foods, root) = world("meal-photo");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, &reply_json(&item("cooked rice", 180.0, 130.0)));
    assert_eq!(run(&c, &msg(4010, "", &["plate.jpg"]).0).await, Outcome::Saved);
    assert!(note(&dir).contains("<!-- pip:meal 4010 -->"), "{}", note(&dir));
    assert_eq!(md_count(&foods), 0);
}

#[tokio::test]
async fn with_no_foods_folder_a_label_is_refused_and_nothing_is_written() {
    let (dir, foods, root) = world("no-dir");
    let c = cfg(&dir, None, Some(stub(&root)));
    set_reply(&root, LABEL);
    let (m, replies) = msg(4020, "", &["label.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(joined(&replies).contains("PIP_FOODS_DIR"), "{}", joined(&replies));
    assert!(!foods.exists());
    assert!(!note(&dir).contains("<!-- pip:meal 4020 -->"));
}

#[tokio::test]
async fn an_estimated_item_says_a_label_makes_it_exact() {
    let (dir, foods, root) = world("estimate");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, &reply_json(&item("dumplings", 420.0, 200.0)));
    let (m, replies) = msg(4030, "24 dumplings", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(joined(&replies).contains("nutrition label makes it exact"), "{}", joined(&replies));
}

fn snapshot(dirs: &[&Path]) -> Vec<(PathBuf, String)> {
    let mut all = Vec::new();
    for d in dirs {
        if let Ok(rd) = fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() {
                    all.push((p.clone(), fs::read_to_string(&p).unwrap_or_default()));
                }
            }
        }
    }
    all.sort();
    all
}

#[tokio::test]
async fn a_meal_photo_captioned_with_a_saved_count_counts_that_food_once() {
    let (dir, foods, root) = world("photo-caption");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    assert_eq!(run(&c, &msg(4001, "", &["label.jpg"]).0).await, Outcome::Saved);
    // The reader sees the photo and estimates the same wontons on the plate (the caption is counted from the label).
    set_reply(&root, &reply_json(&item("pork wontons", 420.0, 230.0)));
    let (m, _) = msg(4002, "24 wontons", &["plate.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir);
    assert!(n.contains("(24 pc, label)"), "{n}");
    let wonton_lines = n.lines().filter(|l| l.starts_with("- ") && !l.contains("unknown") && l.to_lowercase().contains("wontons")).count();
    assert_eq!(wonton_lines, 1, "the photo's estimate of the same food was counted too: {n}");
}

#[tokio::test]
async fn a_renamed_estimate_of_the_counted_food_is_kept_and_shown() {
    let (dir, foods, root) = world("renamed");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    assert_eq!(run(&c, &msg(4001, "", &["label.jpg"]).0).await, Outcome::Saved);
    let both = format!("{},{}", item("Pork wontons in broth", 120.0, 200.0), item("cooked rice", 150.0, 130.0));
    set_reply(&root, &reply_json(&both));
    let (m, _) = msg(4002, "24 wontons", &["plate.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir);
    assert!(n.contains("in broth (estimated)"), "{n}");
    assert!(!n.contains("photo estimate Pork wontons in broth"), "{n}");
    assert!(n.contains("cooked rice"), "{n}");
}

#[tokio::test]
async fn a_recipe_line_naming_a_saved_food_counts_it_from_the_label() {
    let (dir, foods, root) = world("recipe-bridge");
    let recipes = root.join("Recipes");
    fs::create_dir_all(&recipes).unwrap();
    fs::write(recipes.join("Dumpling Night.md"), "# Dumpling Night\n\n## Ingredients\n\n- 8 pork wontons\n").unwrap();
    let mut c = cfg(&dir, Some(&foods), Some(stub(&root)));
    c.recipes_dir = Some(recipes);
    set_reply(&root, LABEL);
    assert_eq!(run(&c, &msg(4001, "", &["label.jpg"]).0).await, Outcome::Saved);
    // The reader is asked about the typed rest (empty here) and says nothing; the recipe's own line is counted from the label.
    set_reply(&root, &reply_json(""));
    let (m, _) = msg(4003, "Dumpling Night", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir);
    assert!(n.contains("(8 pc, label)"), "{n}");
}

#[tokio::test]
async fn one_message_writes_one_thing_and_a_replay_writes_nothing() {
    let with_item = LABEL.replace("\"items\":[]", &format!("\"items\":[{}]", item("pork wontons", 420.0, 230.0)));
    let replies = [LABEL.to_string(), with_item, reply_json(&item("cooked rice", 180.0, 130.0)), reply_json("")];
    for (i, reply) in replies.iter().enumerate() {
        let (dir, foods, root) = world(&format!("invariant-{i}"));
        let c = cfg(&dir, Some(&foods), Some(stub(&root)));
        set_reply(&root, reply);
        let id = 4100 + i as u64;
        let (m, _) = msg(id, "", &["photo.jpg"]);
        run(&c, &m).await;
        let meal = note(&dir).contains(&format!("<!-- pip:meal {id} -->"));
        let food = md_count(&foods) > 0;
        assert!(!(meal && food), "reply {i} wrote a meal and a food: {}", note(&dir));
        let before = snapshot(&[&dir, &foods]);
        run(&c, &m).await;
        assert_eq!(snapshot(&[&dir, &foods]), before, "reply {i}: a replay changed a file");
    }
}

#[tokio::test]
async fn a_food_typed_beside_the_saved_count_is_not_dropped() {
    let (dir, foods, root) = world("typed");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    assert_eq!(run(&c, &msg(4001, "", &["label.jpg"]).0).await, Outcome::Saved);
    set_reply(&root, &reply_json(&item("wonton soup", 300.0, 150.0)));
    let (m, _) = msg(4002, "24 wontons, wonton soup", &["plate.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(note(&dir).contains("wonton soup"), "{}", note(&dir));
}

#[tokio::test]
async fn a_photo_soup_beside_a_saved_count_is_kept_once() {
    let (dir, foods, root) = world("soup");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    assert_eq!(run(&c, &msg(4001, "", &["label.jpg"]).0).await, Outcome::Saved);
    set_reply(&root, &reply_json(&item("Wonton soup with broth", 300.0, 60.0)));
    let (m, _) = msg(4002, "24 wontons", &["plate.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir);
    let counted: Vec<&str> = n.lines().filter(|l| l.contains("(24 pc, label)")).collect();
    assert_eq!(counted.len(), 1, "{n}");
    assert!(counted[0].contains("690 kcal"), "{n}");
    let soup: Vec<&str> = n.lines().filter(|l| l.contains("Wonton soup with broth")).collect();
    assert_eq!(soup.len(), 1, "{n}");
    assert!(soup[0].contains("180 kcal"), "{n}");
    assert!(!n.contains("photo estimate"), "{n}");
    let p = prompts(&root);
    assert!(p.contains("Already counted") && p.contains("Northwind Pork Wontons"), "{p}");
}

#[tokio::test]
async fn a_typed_soup_the_reader_renames_is_kept_and_its_copy_of_the_count_is_not() {
    let (dir, foods, root) = world("soup-typed");
    let c = cfg(&dir, Some(&foods), Some(stub(&root)));
    set_reply(&root, LABEL);
    assert_eq!(run(&c, &msg(4001, "", &["label.jpg"]).0).await, Outcome::Saved);
    let both = format!("{},{}", item("Northwind Pork Wontons", 140.0, 164.0), item("Wonton soup with broth", 300.0, 60.0));
    set_reply(&root, &reply_json(&both));
    let (m, _) = msg(4002, "24 wontons, wonton soup", &["plate.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir);
    assert_eq!(n.lines().filter(|l| l.contains("(24 pc, label)")).count(), 1, "{n}");
    let soup: Vec<&str> = n.lines().filter(|l| l.contains("Wonton soup with broth")).collect();
    assert_eq!(soup.len(), 1, "{n}");
    assert!(soup[0].contains("180 kcal"), "{n}");
    assert!(n.contains("photo estimate Northwind Pork Wontons"), "{n}");
    assert!(!n.lines().any(|l| l.contains("Northwind Pork Wontons") && l.contains("(estimated)")), "{n}");
}
