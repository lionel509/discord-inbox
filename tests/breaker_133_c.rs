//! Breaker tests for #133 unit C (round 1). Scratch dirs live under the temp dir, never a real vault.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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


fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("breaker-133-c-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write_exec(p: &Path, body: &str) {
    fs::write(p, body).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Break 1: a 25-line receipt reply keeps 20 items and reports nothing for the other 5.
#[test]
fn breaker_receipt_over_twenty_items_is_not_silently_dropped() {
    let items: Vec<String> = (0..25)
        .map(|i| format!(r#"{{"name":"food{i}","grams":50,"source":"estimate","kcal_100":100,"protein_100":1,"carbs_100":1,"fat_100":1,"veg":false}}"#))
        .collect();
    let json = format!(r#"{{"items":[{}],"unreadable":[]}}"#, items.join(","));
    let p = discord_inbox::pip_model::parse("receipt photo", &json).expect("parses");
    let accounted = p.items.len() + p.unknown.len();
    assert_eq!(accounted, 25, "25 lines in, {} items and {} unknown out; the rest vanished", p.items.len(), p.unknown.len());
}

/// Break 2: a typed 100 g is logged as the model's 150 g (only marked estimated).
#[tokio::test]
async fn breaker_typed_weight_is_not_replaced_by_the_model() {
    let (dir, root) = world("b-typed");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    let salmon = r#"{"name":"grilled salmon","grams":150,"source":"text","kcal_100":208,"protein_100":20,"carbs_100":0,"fat_100":13,"veg":false}"#;
    set_reply(&root, &reply_json(salmon, ""));
    let (m, _) = msg(4001, "2026-10-27T19:00:00-04:00", "grilled salmon 100g", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-27");
    assert!(n.contains("100 g grilled salmon"), "typed 100 g was replaced:\n{n}");
    assert!(!n.contains("150 g grilled salmon"), "model's 150 g was logged:\n{n}");
}

/// Break 3: a reply that says "scale" for a text-only DM (no photo at all) is logged as a measured weight.
#[tokio::test]
async fn breaker_scale_label_without_any_photo_is_not_measured() {
    let (dir, root) = world("b-scale-nophoto");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    let salmon = r#"{"name":"grilled salmon","grams":450,"source":"scale","kcal_100":208,"protein_100":20,"carbs_100":0,"fat_100":13,"veg":false}"#;
    set_reply(&root, &reply_json(salmon, ""));
    let (m, _) = msg(4002, "2026-10-27T19:05:00-04:00", "grilled salmon", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let n = note(&dir, "2026-10-27");
    assert!(!n.contains("(scale)"), "no photo was sent, yet the line reads as a measured scale weight:\n{n}");
}

/// Break 4: the production helper passes ANTHROPIC_API_KEY through to claude, so a key in the env bills cash.
#[test]
fn breaker_pip_model_sh_does_not_pass_an_api_key_through() {
    let root = scratch("b-key");
    let stub_claude = root.join("claude-stub.sh");
    write_exec(&stub_claude, &format!("#!/bin/sh\nenv > '{}'\ncat >/dev/null\necho '{{\"items\":[],\"unreadable\":[]}}'\n", root.join("env.txt").display()));
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/pip-model.sh");
    let out = Command::new("/bin/sh")
        .arg(script)
        .env("DISCORD_INBOX_CLAUDE", &stub_claude)
        .env("ANTHROPIC_API_KEY", "sk-breaker-not-a-real-key")
        .stdin(Stdio::null())
        .output()
        .expect("script runs");
    assert!(out.status.success(), "script failed: {}", String::from_utf8_lossy(&out.stderr));
    let env = fs::read_to_string(root.join("env.txt")).unwrap_or_default();
    assert!(!env.contains("ANTHROPIC_API_KEY"), "claude would bill the API key (cash), not the subscription; the script passed it through");
}

/// Check 5: photo paths with spaces reach --add-dir intact.
#[test]
fn breaker_pip_model_sh_add_dir_survives_spaces() {
    let root = scratch("b-spaces");
    let stub_claude = root.join("claude-stub.sh");
    write_exec(&stub_claude, &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncat >/dev/null\necho '{{\"items\":[],\"unreadable\":[]}}'\n", root.join("argv.txt").display()));
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/pip-model.sh");
    let out = Command::new("/bin/sh")
        .arg(script)
        .arg("/x/Private Dir/Day One/a b.jpg")
        .arg("/x/Private Dir/Day One/c.jpg")
        .env("DISCORD_INBOX_CLAUDE", &stub_claude)
        .stdin(Stdio::null())
        .output()
        .expect("script runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let argv: Vec<String> = fs::read_to_string(root.join("argv.txt")).unwrap_or_default().lines().map(str::to_string).collect();
    let i = argv.iter().position(|a| a == "--add-dir").expect("no --add-dir");
    assert_eq!(argv.get(i + 1).map(String::as_str), Some("/x/Private Dir/Day One"), "argv: {argv:?}");
}

/// Check 6: a helper that leaves a grandchild holding stdout must not hang the bot past its timeout.
#[tokio::test]
async fn breaker_model_grandchild_cannot_hang_the_bot() {
    let (dir, root) = world("b-grandchild");
    let helper = root.join("helper-grandchild.sh");
    write_exec(&helper, "#!/bin/sh\n(sleep 20) &\nsleep 20\n");
    let c = cfg(&dir, Some(helper), 1);
    let (m, _) = msg(4006, "2026-10-27T19:10:00-04:00", "grilled salmon 100g", &[]);
    let started = Instant::now();
    let out = run(&c, &m).await;
    let took = started.elapsed();
    assert!(took < Duration::from_secs(10), "handler took {took:?} with a 1 s model timeout");
    assert_eq!(out, Outcome::Saved);
}

/// Check 7: a replayed DM with a reader wired logs one meal and does not call the model again.
#[tokio::test]
async fn breaker_replayed_dm_with_model_logs_one_meal() {
    let (dir, root) = world("b-replay");
    let c = cfg(&dir, Some(stub(&root, false, 0)), 5);
    set_reply(&root, &reply_json(&item("grilled salmon", 100.0, true, 208.0), ""));
    let (m, _) = msg(4007, "2026-10-27T19:15:00-04:00", "grilled salmon 100g", &[]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    let calls_before = args_of(&root).len();
    assert_eq!(run(&c, &m).await, Outcome::Skipped);
    assert_eq!(args_of(&root).len(), calls_before, "replay called the model again");
    assert_eq!(daylog::meals(&dir, "2026-10-27").iter().filter(|r| r.msg_id == 4007).count(), 1);
}

/// Check 8: photo-only DM with the helper down logs no meal and says so.
#[tokio::test]
async fn breaker_photo_only_down_logs_no_zero_meal() {
    let (dir, root) = world("b-photo-down");
    let c = cfg(&dir, Some(stub(&root, true, 0)), 5);
    let (m, replies) = msg(4008, "2026-10-27T19:20:00-04:00", "", &["dish.jpg"]);
    assert_eq!(run(&c, &m).await, Outcome::Saved);
    assert!(daylog::meals(&dir, "2026-10-27").iter().all(|r| r.msg_id != 4008), "a 0 kcal meal was written");
    assert!(joined(&replies).contains("reader is down"), "{}", joined(&replies));
}
