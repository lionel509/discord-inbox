//! Breaker tests for #133 round 2 (fix commits b1ed25e..e0bb735). Scratch dirs live under the temp dir, never a real vault.
//! Each test asserts the behaviour the round-1 review asked for. A failing test here is a break.
#![allow(dead_code, unused_imports)]
use discord_inbox::allowlist;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use discord_inbox::pip_model::{self, Source};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAIN: u64 = 100000000000000001;
const MAIN_CH: u64 = 5001;

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

fn world(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("breaker-133-r2-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Nutrition");
    fs::create_dir_all(&dir).unwrap();
    (dir, root)
}

fn set_reply(root: &Path, json: &str) {
    fs::write(root.join("model-out"), json).unwrap();
}

fn reply_json(items: &str, unreadable: &str) -> String {
    format!("{{\"items\":[{items}],\"unreadable\":[{unreadable}]}}")
}

/// A reader item in the round-2 shape (`source` instead of the old `typed` flag).
fn it(name: &str, grams: f64, source: &str, kcal: f64) -> String {
    format!(r#"{{"name":"{name}","grams":{grams},"source":"{source}","kcal_100":{kcal},"protein_100":10,"carbs_100":10,"fat_100":5,"veg":false}}"#)
}

/// A `/bin/sh` helper that prints the reply file. Logs its args and stdin for inspection.
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

fn cfg(dir: &Path, model: Option<PathBuf>) -> Config {
    Config {
        nutrition_dir: dir.to_path_buf(),
        recipes_dir: None,
        low_kcal: 1800,
        kcal_target: 2400,
        model,
        model_timeout: Duration::from_secs(5),
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
        bytes: b"file bytes".to_vec(),
        replies: replies.clone(),
    };
    (m, replies)
}

fn joined(r: &Arc<Mutex<Vec<String>>>) -> String {
    r.lock().unwrap().join("\n")
}

/// Everything the user sees for one DM, through the real flow with the stub reader.
async fn flow(name: &str, text: &str, files: &[&str], reply: &str) -> String {
    let (dir, root) = world(name);
    let c = cfg(&dir, Some(stub(&root)));
    set_reply(&root, reply);
    let (m, replies) = msg(7001, "2026-10-27T19:05:00-04:00", text, files);
    let _ = pip::handle(&allow(), &c, &m).await;
    joined(&replies)
}

/// Digits of `s` with fullwidth digits folded and separators removed, so a card split by spaces, dashes or dots still shows.
fn digit_run(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '０'..='９' => out.push(char::from(b'0' + (ch as u32 - '０' as u32) as u8)),
            '0'..='9' => out.push(ch),
            ' ' | '-' | '.' | '·' | '\u{200b}' | '\u{00a0}' => {}
            _ => out.push('\n'),
        }
    }
    out
}

fn has_card(s: &str) -> bool {
    digit_run(s).split('\n').any(|run| run.len() >= 12)
}

/// Round-1 break 1: a card number in a model name must never reach the log or the reply (parse level).
#[test]
fn r2_card_numbers_in_model_names_never_survive() {
    let names = [
        "VISA 4111-1111-1111-1111 chicken",
        "3782 822463 10005 chicken",
        "card 5555 5555 5555 4444 chicken",
        "４１１１１１１１１１１１１１１１ chicken",
        "4111.1111.1111.1111 chicken",
        "4111\u{00a0}1111\u{00a0}1111\u{00a0}1111 chicken",
        "4111\u{200b}1111\u{200b}1111\u{200b}1111 chicken",
    ];
    let mut leaks = Vec::new();
    for name in names {
        let out = reply_json(&it(name, 100.0, "text", 165.0), "");
        let p = pip_model::parse("chicken 100g", &out).expect("object with items");
        let mut shown: Vec<String> = p.items.iter().map(|i| i.food.name.to_string()).collect();
        shown.extend(p.unknown.iter().map(|u| u.text.clone()));
        if shown.iter().any(|s| has_card(s)) {
            leaks.push(format!("{name:?} -> {shown:?}"));
        }
    }
    assert!(leaks.is_empty(), "card digits survived in: {leaks:#?}");
}

/// Round-1 break 2: a `scale` label with no photo is only the reader's word. A text file is not a photo.
#[tokio::test]
async fn r2_scale_label_needs_a_photo_not_any_attachment() {
    let reply = reply_json(&it("grilled salmon", 450.0, "scale", 208.0), "");
    let out = flow("r2-txt", "grilled salmon", &["notes.txt"], &reply).await;
    assert!(!out.contains("(scale)"), "a text file counted as the scale photo, logged as measured: {out}");
}

/// Round-1 deferred finding 9 (B1/B3), listed in the coder's fix list as "typed weights can't be overridden":
/// a weight he typed is replaced by the reader's number.
#[tokio::test]
async fn r2_typed_weight_is_not_replaced_by_the_reader() {
    let reply = reply_json(&it("chicken breast", 300.0, "text", 165.0), "");
    let out = flow("r2-typed", "100g chicken breast", &[], &reply).await;
    assert!(!out.contains("300 g"), "typed 100 g was replaced by 300 g: {out}");
}

/// Same family: a scale photo in the DM overrides a weight he typed in the text.
#[tokio::test]
async fn r2_scale_photo_does_not_override_typed_weight() {
    let reply = reply_json(&it("chicken breast", 300.0, "scale", 165.0), "");
    let out = flow("r2-scale-over-typed", "100g chicken breast", &["plate.jpg"], &reply).await;
    assert!(!out.contains("300 g"), "typed 100 g was overridden by a scale reading of 300 g: {out}");
}

/// Plain water is genuinely 0 kcal. A DM that is only water must not be answered as "couldn't read that as food".
#[tokio::test]
async fn r2_plain_water_is_not_reported_as_unreadable() {
    let reply = reply_json(&it("water", 250.0, "text", 0.0), "");
    let out = flow("r2-water", "water", &[], &reply).await;
    assert!(!out.contains("couldn"), "water alone was answered as unreadable: {out}");
}

/// Water beside food: the reader's 0 kcal is correct, so the reply must not blame the reader for giving none.
#[tokio::test]
async fn r2_water_beside_food_is_not_blamed_on_the_reader() {
    let reply = reply_json(&format!("{},{}", it("boiled egg", 100.0, "estimate", 155.0), it("water", 250.0, "estimate", 0.0)), "");
    let out = flow("r2-water-food", "2 eggs and a glass of water", &[], &reply).await;
    assert!(!out.contains("gave no calories"), "0 kcal water reported as the reader failing: {out}");
}

/// Items past 20: every line is accounted for, either as an item, a named unknown, or the "and N more" count.
#[test]
fn r2_items_past_the_limit_are_all_accounted_for() {
    for n in [21usize, 31, 35, 60] {
        let items: Vec<String> = (0..n).map(|i| it(&format!("food {i}"), 50.0, "estimate", 100.0)).collect();
        let out = reply_json(&items.join(","), "");
        let p = pip_model::parse("receipt", &out).expect("parses");
        let mut accounted = p.items.len();
        for u in &p.unknown {
            accounted += match u.text.strip_prefix("and ").and_then(|s| s.strip_suffix(" more not counted")) {
                Some(k) => k.parse::<usize>().unwrap_or(1),
                None => 1,
            };
        }
        assert_eq!(accounted, n, "{n} items in, {accounted} accounted for");
    }
}

/// The reader echoes the prompt's example object AFTER its real answer: the answer must win.
#[test]
fn r2_echoed_example_after_the_answer_does_not_win() {
    let answer = reply_json(&it("grilled tilapia", 180.0, "estimate", 96.0), "");
    let example = reply_json(&it("cooked egg noodles", 100.0, "text", 138.0), "");
    let out = format!("{answer}\n\nFormat reference: {example}");
    let p = pip_model::parse("grilled tilapia 180g", &out).expect("parses");
    let names: Vec<String> = p.items.iter().map(|i| i.food.name.to_string()).collect();
    assert_eq!(names, vec!["grilled tilapia".to_string()], "logged the echoed example instead of the answer");
}

/// Prose with braces before and after the answer still parses to the answer.
#[test]
fn r2_braces_in_prose_around_the_answer_still_parse() {
    let answer = reply_json(&it("grilled tilapia", 180.0, "estimate", 96.0), "");
    let out = format!("Read {{roughly}} the plate:\n```json\n{answer}\n```\nWeights are {{approximate}}.");
    let p = pip_model::parse("grilled tilapia", &out).expect("parses");
    assert_eq!(p.items.len(), 1, "unknown: {:?}", p.unknown);
}

/// Every unreadable string the reader gives must be reported, not silently capped.
#[test]
fn r2_unreadable_strings_are_not_silently_capped() {
    let items: Vec<String> = (0..20).map(|i| it(&format!("food {i}"), 50.0, "estimate", 100.0)).collect();
    let unread: Vec<String> = (0..12).map(|i| format!("\"smudged line {i}\"")).collect();
    let out = reply_json(&items.join(","), &unread.join(","));
    let p = pip_model::parse("receipt", &out).expect("parses");
    // Named lines plus the "and N more unreadable" count must account for all 12 (review item 6).
    let named = p.unknown.iter().filter(|u| u.hint == "the reader could not place it" && !u.text.starts_with("and ")).count();
    let counted: usize = p
        .unknown
        .iter()
        .find(|u| u.text.starts_with("and ") && u.text.ends_with(" more unreadable"))
        .and_then(|u| u.text.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert_eq!(named + counted, 12, "12 unreadable lines in; {named} named and {counted} counted; the rest vanished");
}

/// pip-model.sh must not pass a key or a cash-billing provider switch through to `claude -p`.
#[test]
fn r2_wrapper_strips_api_key_and_cash_providers() {
    let root = std::env::temp_dir().join(format!("breaker-133-r2-env-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let fake = root.join("claude");
    let dump = root.join("env-names");
    fs::write(
        &fake,
        format!("#!/bin/sh\nenv | grep -E '^(ANTHROPIC|CLAUDE_CODE)' | cut -d= -f1 > '{}'\n", dump.display()),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/pip-model.sh");
    let _ = Command::new("sh")
        .arg(&script)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &root)
        .env("DISCORD_INBOX_CLAUDE", &fake)
        .env("ANTHROPIC_API_KEY", "dummy")
        .env("ANTHROPIC_AUTH_TOKEN", "dummy")
        .env("ANTHROPIC_BASE_URL", "http://127.0.0.1:9")
        .env("CLAUDE_CODE_USE_BEDROCK", "1")
        .env("CLAUDE_CODE_USE_VERTEX", "1")
        .stdin(Stdio::null())
        .output();
    let names = fs::read_to_string(&dump).unwrap_or_default();
    for key in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX"] {
        assert!(!names.lines().any(|l| l == key), "{key} reached claude -p; names seen: {names:?}");
    }
}
