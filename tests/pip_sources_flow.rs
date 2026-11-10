//! #159 U4: bought products through `pip::handle` with the reader stubbed by a `/bin/sh` helper (`PIP_MODEL`), as in
//! `tests/pip_foods_flow.rs`. Made-up folders under the temp dir; no live model.
use discord_inbox::pantry::{self, Product};
use discord_inbox::pip::Config;
use discord_inbox::ingest::Outcome;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::Inbound;
use discord_inbox::pip;

const MAIN: u64 = 100000000000000001;
const MAIN_CH: u64 = 5001;
const LABEL_SOURDOUGH: &str = r#"{"label":{"name":"Farmhouse Sourdough Bread","serving_g":43,"serving_pc":1,"kcal":265,"protein":9,"carbs":49,"fat":3},"items":[],"unreadable":[]}"#;

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

/// Nutrition dir, foods dir, pantry note path, and the root (helper, its output and its call log live there).
fn world(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pip-sources-flow-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Nutrition");
    let foods = root.join("Private").join("Foods");
    let pantry_note = root.join("Kitchen").join("Pantry").join("On Hand.md");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(pantry_note.parent().unwrap()).unwrap();
    (dir, foods, pantry_note, root)
}

fn write_pantry(note: &Path, products: &[Product]) {
    fs::write(note, pantry::render_with(&[], products, "2026-10-27")).unwrap();
}

fn sourdough(bought: &str) -> Product {
    Product { name: "Farmhouse Sourdough Bread".into(), brand: "Pepperidge Farm".into(), size: "24 oz".into(), bought: bought.into() }
}

fn set_reply(root: &Path, json: &str) {
    fs::write(root.join("model-out"), json).unwrap();
}

fn reply_json(items: &str) -> String {
    format!("{{\"items\":[{items}],\"unreadable\":[]}}")
}

/// One reader item, naming a bought product when `product` is `Some`.
fn bread_item(product: Option<&str>) -> String {
    let base = r#"{"name":"sourdough bread","grams":43,"typed":false,"kcal_100":265,"protein_100":9,"carbs_100":49,"fat_100":3,"veg":false"#;
    match product {
        Some(p) => format!("{base},\"product\":\"{p}\"}}"),
        None => format!("{base}}}"),
    }
}

/// A `/bin/sh` reader: appends its arg count to a call log, keeps its stdin, then prints the reply file.
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

fn cfg(dir: &Path, foods: Option<&Path>, pantry_note: Option<&Path>, model: Option<PathBuf>) -> Config {
    Config {
        nutrition_dir: dir.to_path_buf(),
        recipes_dir: None,
        foods_dir: foods.map(Path::to_path_buf),
        low_kcal: 1800,
        kcal_target: 2400,
        model,
        model_timeout: Duration::from_secs(5),
        pantry_note: pantry_note.map(Path::to_path_buf),
        dish_paths: Vec::new(),
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

#[test]
fn env_parses_pantry_note_and_dish_paths() {
    let (dir, _, _, root) = world("env");
    std::env::set_var("PIP_NUTRITION_DIR", &dir);
    let pantry_note = root.join("Kitchen/Pantry/On Hand.md");
    let dishes = format!("{}:{}", root.join("Kitchen/Recipes").display(), root.join("Kitchen/Soup.md").display());
    std::env::set_var("PIP_PANTRY_NOTE", &pantry_note);
    std::env::set_var("PIP_DISH_NOTES", &dishes);
    let c = Config::from_env().expect("absolute paths parse");
    assert_eq!(c.pantry_note.as_deref(), Some(pantry_note.as_path()));
    assert_eq!(c.dish_paths.len(), 2);
    std::env::set_var("PIP_PANTRY_NOTE", "relative/On Hand.md");
    assert!(Config::from_env().is_err(), "a relative pantry note is refused");
    std::env::set_var("PIP_PANTRY_NOTE", &pantry_note);
    std::env::set_var("PIP_DISH_NOTES", "relative/Recipes");
    assert!(Config::from_env().is_err(), "a relative dish path is refused");
    std::env::remove_var("PIP_PANTRY_NOTE");
    std::env::remove_var("PIP_DISH_NOTES");
    std::env::remove_var("PIP_NUTRITION_DIR");
}

#[tokio::test]
async fn a_bought_product_names_its_source_and_lands_in_the_day_note() {
    let (dir, _, pantry_note, root) = world("bought");
    write_pantry(&pantry_note, &[sourdough("2026-10-27")]);
    let c = cfg(&dir, None, Some(&pantry_note), Some(stub(&root)));
    set_reply(&root, &reply_json(&bread_item(Some("Farmhouse Sourdough Bread"))));
    let (m, replies) = msg(4101, "1 slice sourdough", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let r = joined(&replies);
    assert!(r.contains("bought: Farmhouse Sourdough Bread)"), "{r}");
    assert!(r.contains("From what you bought"), "{r}");
    assert!(note(&dir).contains("bought: Farmhouse Sourdough Bread"), "{}", note(&dir));
    assert!(prompts(&root).contains("Farmhouse Sourdough Bread"), "the reader was offered the product");
}

#[tokio::test]
async fn today_and_undo_work_after_a_sourced_meal() {
    let (dir, _, pantry_note, root) = world("undo");
    write_pantry(&pantry_note, &[sourdough("2026-10-27")]);
    let c = cfg(&dir, None, Some(&pantry_note), Some(stub(&root)));
    set_reply(&root, &reply_json(&bread_item(Some("Farmhouse Sourdough Bread"))));
    assert_eq!(run(&c, &msg(4102, "1 slice sourdough", &[]).0).await, Outcome::Saved);
    assert!(note(&dir).contains("<!-- pip:meal 4102 -->"), "{}", note(&dir));
    let (today, replies) = msg(4103, "today", &[]);
    run(&c, &today).await;
    assert!(joined(&replies).contains("kcal"), "{}", joined(&replies));
    assert_eq!(run(&c, &msg(4104, "undo", &[]).0).await, Outcome::Saved);
    assert!(!note(&dir).contains("<!-- pip:meal 4102 -->"), "{}", note(&dir));
}

#[tokio::test]
async fn reader_down_is_table_only() {
    let (dir, _, pantry_note, root) = world("down");
    write_pantry(&pantry_note, &[sourdough("2026-10-27")]);
    let c = cfg(&dir, None, Some(&pantry_note), Some(stub(&root)));
    set_reply(&root, "not json at all");
    let (m, replies) = msg(4105, "2 eggs", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let r = joined(&replies);
    assert!(!r.contains("bought:"), "{r}");
    assert!(!r.contains("From what you bought"), "{r}");
}

#[tokio::test]
async fn product_named_like_a_saved_food_is_not_offered() {
    let (dir, foods, pantry_note, root) = world("saved");
    write_pantry(&pantry_note, &[sourdough("2026-10-27")]);
    let c = cfg(&dir, Some(&foods), Some(&pantry_note), Some(stub(&root)));
    set_reply(&root, LABEL_SOURDOUGH);
    let (label, _) = msg(4106, "", &["label.jpg"]);
    assert_eq!(run(&c, &label).await, Outcome::Saved);
    fs::remove_file(root.join("model-stdin")).ok();
    set_reply(&root, &reply_json(&bread_item(None)));
    assert_eq!(run(&c, &msg(4107, "1 slice sourdough", &[]).0).await, Outcome::Saved);
    assert!(!prompts(&root).contains("Bought products"), "the saved label wins; the product is not offered: {}", prompts(&root));
}
