//! U5 + U6: one DM from start to finish through `pip::handle`, with a fake `Inbound` that records cards. Scratch dirs are `…/Private/…`; recipes are made-up fixtures.
use chrono::NaiveDate;
use discord_inbox::allowlist;
use discord_inbox::checkin;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use discord_inbox::rich::Rich;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAIN: u64 = 100000000000000001;
const DAY: &str = "2026-10-26";
const RECEIVED: &str = "2026-10-26T19:30:00-04:00";

const SALMON: &str = "---\ntags:\n  - food\n---\n# 🐟 Gochujang Honey Garlic Salmon\n\n## Per 100 g salmon portion\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n| Honey | 12 |\n| Gochujang | 4 |\n| Garlic | 3 |\n| Sesame oil | 2 |\n| Rice vinegar | 3 |\n| Scallion | 9 |\n";
const CHICKEN: &str = "# Honey Garlic Chicken\n\n## Ingredients\n\n- 150 g chicken thigh\n- 15 g honey\n- 3 g garlic\n";

struct Live {
    id: u64,
    received: String,
    text: String,
    files: Vec<String>,
    replies: Mutex<Vec<String>>,
    cards: Mutex<Vec<Rich>>,
}

impl Live {
    fn new(id: u64, received: &str, text: &str, files: &[&str]) -> Live {
        Live {
            id,
            received: received.into(),
            text: text.into(),
            files: files.iter().map(|s| s.to_string()).collect(),
            replies: Mutex::new(Vec::new()),
            cards: Mutex::new(Vec::new()),
        }
    }
}

impl Inbound for Live {
    fn author_id(&self) -> u64 {
        MAIN
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        5001
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
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(vec![0x89, b'P', b'N', b'G'])
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
    async fn reply_rich(&self, r: &Rich) -> Result<(), String> {
        self.cards.lock().unwrap().push(r.clone());
        Ok(())
    }
}

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pip-v2-flow-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let recipes = root.join("Private").join("Recipes");
    fs::create_dir_all(&recipes).unwrap();
    fs::write(recipes.join("Gochujang Honey Garlic Salmon.md"), SALMON).unwrap();
    fs::write(recipes.join("Honey Garlic Chicken.md"), CHICKEN).unwrap();
    (root.join("Private").join("Nutrition"), recipes)
}

fn allow() -> allowlist::Allowlist {
    allowlist::parse(&format!("{MAIN}\n")).unwrap()
}

fn cfg(nutrition: &Path, recipes: &Path) -> Config {
    Config {
        nutrition_dir: nutrition.to_path_buf(),
        recipes_dir: Some(recipes.to_path_buf()),
        foods_dir: None,        low_kcal: 1800,
        kcal_target: 2400,
        model: None,
        model_timeout: std::time::Duration::from_secs(90),
    }
}

async fn send(nutrition: &Path, recipes: &Path, m: &Live) -> Outcome {
    pip::handle(&allow(), &cfg(nutrition, recipes), m).await
}

fn note_text(nutrition: &Path) -> String {
    fs::read_to_string(daylog::note_path(nutrition, DAY)).unwrap_or_default()
}

fn meal_blocks(nutrition: &Path) -> usize {
    note_text(nutrition).matches("<!-- pip:meal ").count()
}

fn kcal_today(nutrition: &Path) -> f64 {
    daylog::meals(nutrition, DAY).iter().map(|m| m.kcal).sum()
}

const ACCEPTANCE: &str = "Cooked white rice:1/2 cup\nFarmed Atlantic salmon 100 grams\nthe samon made with the hot honey garlic glaze";

#[tokio::test]
async fn acceptance_message_counts_the_recipe_and_typed_lines() {
    let (nut, rec) = scratch("accept");
    let m = Live::new(1, RECEIVED, ACCEPTANCE, &[]);
    assert_eq!(send(&nut, &rec, &m).await, Outcome::Saved);
    let text = note_text(&nut);
    assert_eq!(meal_blocks(&nut), 1, "exactly one meal block");
    assert!(text.contains("Gochujang Honey Garlic Salmon ×1"));
    assert!(text.contains("100 g salmon") || text.contains("100 g Atlantic salmon"), "{text}");
    assert!(text.contains("79 g cooked rice"), "{text}");
    assert!(text.contains("12 g honey"), "{text}");
    assert!(!text.contains("(default"), "no garlic default: {text}");
    assert!(!text.contains("cabbage"));
    let kcal = kcal_today(&nut);
    assert!((380.0..=395.0).contains(&kcal), "kcal {kcal}");
    let cards = m.cards.lock().unwrap();
    let card = cards.last().expect("a meal card");
    assert!(card.title.ends_with(" kcal"));
    for f in ["Protein", "Carbs", "Fat", "Veg"] {
        assert!(card.fields.iter().any(|(n, _, _)| n == f), "field {f}");
    }
}

#[tokio::test]
async fn adding_cabbage_marks_veg_and_counts_it() {
    let (nut, rec) = scratch("cabbage");
    let text = format!("{ACCEPTANCE}\ncabbage 85g");
    let m = Live::new(2, RECEIVED, &text, &[]);
    send(&nut, &rec, &m).await;
    assert!(note_text(&nut).contains("85 g cabbage"));
    let kcal = kcal_today(&nut);
    assert!((400.0..=415.0).contains(&kcal), "kcal {kcal}");
}

#[tokio::test]
async fn a_typed_salmon_weight_scales_the_recipe() {
    let (nut, rec) = scratch("weight");
    let text = "Cooked white rice:1/2 cup\nFarmed Atlantic salmon 150 grams\nthe samon made with the hot honey garlic glaze";
    let m = Live::new(3, RECEIVED, text, &[]);
    send(&nut, &rec, &m).await;
    assert!(note_text(&nut).contains("18 g honey"));
    let kcal = kcal_today(&nut);
    assert!((520.0..=536.0).contains(&kcal), "kcal {kcal}");
}

#[tokio::test]
async fn glaze_alone_says_it_assumed_one_serving() {
    let (nut, rec) = scratch("assumed");
    let m = Live::new(4, RECEIVED, "the samon made with the hot honey garlic glaze", &[]);
    send(&nut, &rec, &m).await;
    let cards = m.cards.lock().unwrap();
    assert!(cards.last().unwrap().text.contains("Assumed 1 serving"));
}

#[tokio::test]
async fn non_food_messages_are_notes_and_keep_the_checkin_due() {
    let (nut, rec) = scratch("chat");
    let words = ["test", "hi", "hello pip", "ok", "thanks", "lol", "good morning", "yo", "did it work"];
    let mut seed: u64 = 0x5EED_222;
    let now = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap().and_hms_opt(21, 5, 0).unwrap();
    for i in 0..200u64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let n = 1 + (seed >> 33) as usize % 3;
        let mut parts = Vec::new();
        for k in 0..n {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            parts.push(words[((seed >> 33) as usize + k) % words.len()]);
        }
        let sep = if (seed >> 40) % 2 == 0 { " " } else { "\n" };
        let text = parts.join(sep);
        let m = Live::new(1000 + i, RECEIVED, &text, &[]);
        send(&nut, &rec, &m).await;
        assert_eq!(meal_blocks(&nut), 0, "{text:?} became a meal");
        assert!(!daylog::has_meal(&nut, DAY), "{text:?}");
        assert!(checkin::due(&nut, now).is_some(), "{text:?} cancelled the check-in");
    }
}

#[tokio::test]
async fn today_week_and_help_write_no_meal() {
    let (nut, rec) = scratch("cmds");
    for (i, c) in ["today", "/Today", "week", "help"].iter().enumerate() {
        let m = Live::new(50 + i as u64, RECEIVED, c, &[]);
        send(&nut, &rec, &m).await;
    }
    assert_eq!(meal_blocks(&nut), 0);
}

#[tokio::test]
async fn today_card_carries_a_png_chart() {
    let (nut, rec) = scratch("png");
    let m = Live::new(60, RECEIVED, "today", &[]);
    send(&nut, &rec, &m).await;
    let cards = m.cards.lock().unwrap();
    let (name, bytes) = cards.last().unwrap().image.clone().expect("an image");
    assert!(name.ends_with(".png"));
    assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
}

#[tokio::test]
async fn undo_is_idempotent_across_a_replay() {
    let (nut, rec) = scratch("undo");
    send(&nut, &rec, &Live::new(70, RECEIVED, "salmon 100g", &[])).await;
    send(&nut, &rec, &Live::new(71, RECEIVED, "cabbage 85g", &[])).await;
    assert_eq!(meal_blocks(&nut), 2);
    send(&nut, &rec, &Live::new(72, RECEIVED, "undo", &[])).await;
    assert_eq!(daylog::meals(&nut, DAY).len(), 1);
    let replay = send(&nut, &rec, &Live::new(72, RECEIVED, "undo", &[])).await;
    assert_eq!(replay, Outcome::Skipped);
    assert_eq!(daylog::meals(&nut, DAY).len(), 1, "one undo, not two");
}

#[tokio::test]
async fn fix_replaces_the_last_meal_only() {
    let (nut, rec) = scratch("fix");
    send(&nut, &rec, &Live::new(80, RECEIVED, "salmon 100g", &[])).await;
    send(&nut, &rec, &Live::new(81, RECEIVED, "cabbage 85g", &[])).await;
    send(&nut, &rec, &Live::new(82, RECEIVED, "fix salmon 120g", &[])).await;
    let text = note_text(&nut);
    assert!(text.contains("<!-- pip:undone 81 -->"));
    assert!(text.contains("120 g salmon"));
    let kcal = kcal_today(&nut);
    assert!((kcal - 457.6).abs() < 0.5, "kcal {kcal}");
}

#[tokio::test]
async fn fix_of_non_food_changes_nothing() {
    let (nut, rec) = scratch("fixbad");
    send(&nut, &rec, &Live::new(90, RECEIVED, "salmon 100g", &[])).await;
    let before = note_text(&nut);
    send(&nut, &rec, &Live::new(91, RECEIVED, "fix test", &[])).await;
    assert_eq!(note_text(&nut), before);
}

#[tokio::test]
async fn a_photo_alone_is_a_note_with_the_photo_saved() {
    let (nut, rec) = scratch("photo");
    let m = Live::new(95, RECEIVED, "", &["lunch.jpg"]);
    send(&nut, &rec, &m).await;
    assert_eq!(meal_blocks(&nut), 0);
    assert!(m.replies.lock().unwrap().iter().any(|r| r.contains("don't guess from photos")));
}
