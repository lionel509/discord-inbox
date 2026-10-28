//! #130 unit C: meal DMs, photos and receipts through `pip::handle`, with the model reader stubbed by a `/bin/sh` helper
//! (`PIP_MODEL`). Scratch dirs are `…/Private/Nutrition`, never a real vault.
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
const RECEIPT_EN: &[u8] = include_bytes!("fixtures/receipt_en.png");
const COD: &str = "# Baked Cod\n\n## Ingredients\n\n- 1 cod fillet, thawed\n- Neutral oil, a teaspoon\n";

struct Msg {
    id: u64,
    received: String,
    text: String,
    files: Vec<String>,
    bytes: Vec<u8>,
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
        Ok(self.bytes.clone())
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

/// A fresh scratch nutrition dir, and the root above it (the helper, its output and its call log live there).
fn world(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pip-model-flow-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Nutrition");
    fs::create_dir_all(&dir).unwrap();
    (dir, root)
}

/// Sets what the helper prints on its next call (a JSON reply).
fn set_reply(root: &Path, json: &str) {
    fs::write(root.join("model-out"), json).unwrap();
}

fn reply_json(items: &str, unreadable: &str) -> String {
    format!("{{\"items\":[{items}],\"unreadable\":[{unreadable}]}}")
}

fn item(name: &str, grams: f64, typed: bool, kcal: f64) -> String {
    format!(
        r#"{{"name":"{name}","grams":{grams},"typed":{typed},"kcal_100":{kcal},"protein_100":10,"carbs_100":10,"fat_100":5,"veg":false}}"#
    )
}

/// A `/bin/sh` model helper: logs its args and stdin, sleeps `sleep` seconds, then prints the reply. `down` exits 1.
fn stub(root: &Path, down: bool, sleep: u64) -> PathBuf {
    let p = root.join(format!("helper-{down}-{sleep}.sh"));
    let body = if down {
        "#!/bin/sh\ncat >/dev/null; exit 1\n".to_string()
    } else {
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{args}'\ncat >> '{stdin}'\nsleep {sleep}\ncat '{out}'\n",
            args = root.join("model-args").display(),
            stdin = root.join("model-stdin").display(),
            out = root.join("model-out").display(),
        )
    };
    fs::write(&p, body).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn args_of(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join("model-args")).unwrap_or_default().lines().map(str::to_string).collect()
}

fn stdin_of(root: &Path) -> String {
    fs::read_to_string(root.join("model-stdin")).unwrap_or_default()
}

fn cfg(dir: &Path, model: Option<PathBuf>, timeout_secs: u64) -> Config {
    Config {
        nutrition_dir: dir.to_path_buf(),
        recipes_dir: None,
        low_kcal: 1800,
        kcal_target: 2400,
        model,
        model_timeout: Duration::from_secs(timeout_secs),
    }
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn msg(id: u64, received: &str, text: &str, files: &[&str]) -> (Msg, Arc<Mutex<Vec<String>>>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg {
        id,
        received: received.to_string(),
        text: text.to_string(),
        files: files.iter().map(|f| f.to_string()).collect(),
        bytes: b"jpeg bytes".to_vec(),
        replies: replies.clone(),
    };
    (m, replies)
}

fn receipt_msg(id: u64, received: &str) -> (Msg, Arc<Mutex<Vec<String>>>) {
    let (mut m, r) = msg(id, received, "", &["receipt.png"]);
    m.bytes = RECEIPT_EN.to_vec();
    (m, r)
}

async fn run(c: &Config, m: &Msg) -> Outcome {
    pip::handle(&allow(), c, m).await
}

fn note(dir: &Path, day: &str) -> String {
    fs::read_to_string(daylog::note_path(dir, day)).unwrap_or_default()
}

fn joined(r: &Arc<Mutex<Vec<String>>>) -> String {
    r.lock().unwrap().join("\n")
}

const DOWN_LINE: &str = "reader down: photos not read, counted from the table only";

#[tokio::test]
async fn typo_two_foods_logs_both_with_typed_weights() {
    let (dir, root) = world("typo");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    set_reply(&root, &reply_json(&format!("{},{}", item("cooked egg noodles", 100.0, true, 138.0), item("chicken breast", 100.0, true, 120.0)), ""));
    let (m, _) = msg(3001, "2026-10-26T12:00:00-04:00", "100 grnas of noodle 100 grams of chicken", &["a.jpg", "b.jpg", "c.jpg", "d.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(n.contains("<!-- pip:meal 3001 -->"), "{n}");
    assert!(n.contains("100 g cooked egg noodles · 138 kcal"), "{n}");
    assert!(n.contains("100 g chicken breast · 120 kcal"), "{n}");
    assert!(!n.contains("(estimated)"), "{n}");
    let args = args_of(&root);
    assert_eq!(args.len(), 4, "{args:?}");
    assert!(args.iter().all(|a| a.starts_with('/') && Path::new(a).exists()), "{args:?}");
}

#[tokio::test]
async fn photo_only_meal_is_read_and_marked_estimated() {
    let (dir, root) = world("photo-only");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    set_reply(&root, &reply_json(&item("cooked egg noodles", 100.0, false, 138.0), ""));
    let (m, replies) = msg(3002, "2026-10-26T12:00:00-04:00", "", &["a.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(n.contains("<!-- pip:meal 3002 -->"), "{n}");
    assert!(n.contains("100 g cooked egg noodles (estimated) · 138 kcal"), "{n}");
    assert!(stdin_of(&root).contains("Photos: 1 attached"), "{}", stdin_of(&root));
    assert!(!joined(&replies).contains("No food I could read"), "{}", joined(&replies));
}

#[tokio::test]
async fn receipt_photo_logs_items_at_send_time() {
    let (dir, root) = world("receipt");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    set_reply(&root, &reply_json(&format!("{},{}", item("fried chicken", 200.0, false, 250.0), item("cooked rice", 158.0, false, 130.0)), ""));
    let (m, _) = receipt_msg(3003, "2026-10-26T22:55:00-04:00");
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(n.contains("### 22:55 · msg 3003"), "{n}");
    assert!(n.contains("<!-- pip:meal 3003 -->"), "{n}");
    assert_eq!(n.matches("(estimated)").count(), 2, "{n}");
}

#[tokio::test]
async fn unreadable_never_logs_zero() {
    let (dir, root) = world("unreadable");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    set_reply(&root, &reply_json("", "\"grnas noodle\""));
    let (m, replies) = msg(3004, "2026-10-26T12:00:00-04:00", "grnas noodle", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(!n.contains("<!-- pip:meal"), "{n}");
    assert!(n.contains("<!-- pip:note 3004 -->"), "{n}");
    assert!(daylog::meals(&dir, "2026-10-26").is_empty());
    let r = joined(&replies);
    assert!(r.contains("couldn't read that as food"), "{r}");
    assert!(r.contains("grnas noodle"), "{r}");
}

#[tokio::test]
async fn chat_line_is_a_note() {
    let (dir, root) = world("chat");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    set_reply(&root, &reply_json("", ""));
    let (m, replies) = msg(3005, "2026-10-26T12:00:00-04:00", "thanks pip", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(note(&dir, "2026-10-26").contains("<!-- pip:note 3005 -->"));
    assert!(!note(&dir, "2026-10-26").contains("<!-- pip:meal"));
    assert!(joined(&replies).contains("No food in that"), "{}", joined(&replies));
}

#[tokio::test]
async fn model_down_with_table_items_logs_table_only() {
    let (dir, root) = world("down-table");
    let c = cfg(&dir, Some(stub(&root, true, 0)), 5);
    let (m, replies) = msg(3006, "2026-10-26T12:00:00-04:00", "salmon 150g", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(n.contains("<!-- pip:meal 3006 -->"), "{n}");
    assert!(n.contains("312 kcal"), "{n}");
    assert!(n.contains(DOWN_LINE), "{n}");
    assert!(joined(&replies).contains("Reader: down"), "{}", joined(&replies));
}

#[tokio::test]
async fn model_down_with_nothing_matchable_logs_note_and_says_so() {
    let (dir, root) = world("down-nothing");
    let c = cfg(&dir, Some(stub(&root, true, 0)), 5);
    let (m, replies) = msg(3007, "2026-10-26T12:00:00-04:00", "100 grnas of noodle", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(!n.contains("<!-- pip:meal"), "{n}");
    assert!(n.contains("<!-- pip:note 3007 -->"), "{n}");
    assert!(joined(&replies).contains("reader is down"), "{}", joined(&replies));
    // A Down DM never stays owed: a replay skips it and nothing is left to retry.
    assert!(daylog::owed_before(&dir, "2026-10-27", u64::MAX).is_none());
}

#[tokio::test]
async fn model_timeout_counts_as_down() {
    let (dir, root) = world("timeout");
    set_reply(&root, &reply_json(&item("cooked egg noodles", 100.0, false, 138.0), ""));
    let c = cfg(&dir, Some(stub(&root, false, 3)), 1);
    let (m, replies) = msg(3008, "2026-10-26T12:00:00-04:00", "salmon 150g", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(n.contains("312 kcal"), "{n}");
    assert!(n.contains(DOWN_LINE), "{n}");
    assert!(joined(&replies).contains("Reader: down"), "{}", joined(&replies));
}

#[tokio::test]
async fn unreadable_card_number_never_reaches_note_or_reply() {
    let (dir, root) = world("card");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    set_reply(&root, &reply_json("", "\"VISA 4111111111111111\""));
    let (m, replies) = msg(3009, "2026-10-26T12:00:00-04:00", "grnas noodle", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(!note(&dir, "2026-10-26").contains("4111111111111111"));
    assert!(!joined(&replies).contains("4111111111111111"), "{}", joined(&replies));
}

#[tokio::test]
async fn fix_through_the_model_replaces_the_last_meal() {
    let (dir, root) = world("fix");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    set_reply(&root, &reply_json(&item("salmon", 150.0, true, 208.0), ""));
    let (m, _) = msg(3010, "2026-10-26T12:00:00-04:00", "salmon 150g", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(note(&dir, "2026-10-26").contains("<!-- pip:meal 3010 -->"));

    set_reply(&root, &reply_json(&item("chicken breast", 150.0, true, 120.0), ""));
    let (f, _) = msg(3011, "2026-10-26T12:05:00-04:00", "fix 150 grams chicken breast", &[]);
    assert_eq!(run(&c, &f).await, Outcome::Saved);
    let n = note(&dir, "2026-10-26");
    assert!(!n.contains("<!-- pip:meal 3010 -->"), "{n}");
    assert!(n.contains("<!-- pip:meal 3011 -->"), "{n}");
    assert!(n.contains("chicken breast"), "{n}");
}

#[tokio::test]
async fn recipe_name_sends_only_the_typed_rest_to_the_model() {
    let (dir, root) = world("recipe");
    let recipes = root.join("Private").join("Recipes");
    fs::create_dir_all(&recipes).unwrap();
    fs::write(recipes.join("Baked Cod.md"), COD).unwrap();
    let mut c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    c.recipes_dir = Some(recipes);
    set_reply(&root, &reply_json(&item("salmon", 100.0, true, 208.0), ""));
    let (m, _) = msg(3012, "2026-10-26T12:00:00-04:00", "Baked Cod with salmon 100g", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let prompt = stdin_of(&root);
    assert!(prompt.contains("salmon 100g"), "{prompt}");
    assert!(!prompt.contains("Baked Cod"), "the recipe name stays local: {prompt}");
    assert!(note(&dir, "2026-10-26").contains("<!-- pip:meal 3012 -->"));
}

#[tokio::test]
async fn no_model_configured_keeps_table_behaviour() {
    let (dir, root) = world("no-model");
    let c = cfg(&dir, None, 5);
    let (m, replies) = msg(3013, "2026-10-26T12:00:00-04:00", "salmon 150g", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(note(&dir, "2026-10-26").contains("312 kcal"));
    assert!(!root.join("model-args").exists(), "no helper call without PIP_MODEL");
    assert!(!joined(&replies).contains("Reader:"), "{}", joined(&replies));
}

#[tokio::test]
async fn property_no_meal_block_without_items() {
    let (dir, root) = world("property");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    let words = ["grnas", "noodle", "chiken", "thanks", "hello", "pip", "maybe", "lunch", "rice", "2 eggs"];
    let mut s: u64 = 0xC0FF_EE00_1234_5678;
    let mut next = move || {
        s = s.wrapping_mul(6364136223846979645).wrapping_add(1442695040888963407);
        s >> 33
    };
    let n_dms = 200;
    for i in 0..n_dms {
        let n_words = 1 + (next() % 3) as usize;
        let text: Vec<&str> = (0..n_words).map(|_| words[(next() % words.len() as u64) as usize]).collect();
        let text = text.join(" ");
        let n_photos = (next() % 3) as usize;
        let files: Vec<String> = (0..n_photos).map(|k| format!("p{k}.jpg")).collect();
        let unreadable = if next() % 2 == 0 { "\"something odd\"" } else { "" };
        set_reply(&root, &reply_json("", unreadable));
        let file_refs: Vec<&str> = files.iter().map(|f| f.as_str()).collect();
        let (m, replies) = msg(4000 + i, "2026-10-26T12:00:00-04:00", &text, &file_refs);
        assert_eq!(run(&c, &m).await, Outcome::Saved, "dm {i}: {text}");
        assert!(!joined(&replies).is_empty(), "dm {i} got no reply");
    }
    let n = note(&dir, "2026-10-26");
    assert!(!n.contains("<!-- pip:meal"), "a meal block was written with no items");
    assert_eq!(n.matches("<!-- pip:note ").count(), n_dms as usize);
}

#[tokio::test]
async fn scale_photos_weigh_the_food_and_veg_counts() {
    // The amendment on #130: a scale photo is a weight he measured. Cabbage is the later reading minus the empty bowl.
    let (dir, root) = world("scale");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    let cabbage = r#"{"name":"cabbage","grams":87,"source":"scale","kcal_100":25,"protein_100":1.3,"carbs_100":5.8,"fat_100":0.1,"veg":true}"#;
    let items = format!(
        "{},{},{},{}",
        item("cooked egg noodles", 100.0, true, 138.0),
        item("chicken breast", 100.0, true, 120.0),
        cabbage,
        item("onion", 60.0, false, 40.0)
    );
    set_reply(&root, &reply_json(&items, ""));
    let (m, _) = msg(3014, "2026-10-27T19:00:00-04:00", "100 grnas of noodle 100 grams of chicken", &["dish.jpg", "bowl-empty.jpg", "onion.jpg", "bowl-cabbage.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-27");
    assert!(n.contains("<!-- pip:meal 3014 -->"), "{n}");
    assert!(n.contains("87 g cabbage (scale)"), "{n}");
    assert!(!n.contains("cabbage (estimated)"), "a scale reading is measured: {n}");
    assert!(n.contains("100 g cooked egg noodles · 138 kcal"), "{n}");
    assert!(n.contains("100 g chicken breast · 120 kcal"), "{n}");
    assert!(n.contains("60 g onion (estimated)"), "{n}");
    assert!(n.contains("veg yes"), "{n}");
    let rows = daylog::meals(&dir, "2026-10-27");
    assert!(rows.iter().any(|r| r.msg_id == 3014 && r.kcal > 0.0), "no kcal logged");
    assert!(stdin_of(&root).contains("Photos: 4 attached"), "{}", stdin_of(&root));
    assert!(stdin_of(&root).contains("\"source\": \"scale\""), "{}", stdin_of(&root));
}

#[test]
fn scale_reading_is_bounded_at_5kg() {
    use discord_inbox::pip_model::{self, Source};
    let ok = pip_model::enforce("bowl photo", "cabbage", 4999.0, Source::Scale, 25.0, 1.0, 5.0, 0.1, true);
    assert!(ok.as_ref().is_ok_and(|i| i.scale && !i.estimated), "{ok:?}");
    let zero = pip_model::enforce("bowl photo", "cabbage", 0.0, Source::Scale, 25.0, 1.0, 5.0, 0.1, true);
    assert!(zero.is_err(), "0 g scale reading must be rejected");
    let heavy = pip_model::enforce("bowl photo", "cabbage", 6000.0, Source::Scale, 25.0, 1.0, 5.0, 0.1, true);
    assert_eq!(heavy.err().map(|u| u.hint), Some("the reader gave an impossible number".to_string()));
}

#[test]
fn text_weight_not_in_the_message_is_downgraded_to_estimate() {
    use discord_inbox::pip_model::{self, Source};
    let i = pip_model::enforce("salmon 150g", "salmon", 120.0, Source::Text, 208.0, 20.0, 0.0, 13.0, false).unwrap();
    assert!(i.estimated && !i.scale, "120 g is not in the text, so it is an estimate");
    let t = pip_model::enforce("salmon 150g", "salmon", 150.0, Source::Text, 208.0, 20.0, 0.0, 13.0, false).unwrap();
    assert!(!t.estimated && !t.scale, "150 g is typed");
}
