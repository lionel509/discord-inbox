//! The verdict chain: ScaleDown Extract through OpenRouter first, then Claude Haiku direct. Every model
//! and curl call here goes to a stub script; no test reaches the network.
use discord_inbox::dump::Incoming;
use discord_inbox::insight::{self, Insight, Model};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

/// Process-global env vars: every test that reads or writes them holds this.
static LOCK: Mutex<()> = Mutex::new(());

const OPENROUTER: &str = "https://openrouter.ai/api/v1/chat/completions";

fn cap() -> PathBuf {
    std::env::temp_dir().join(format!("di-chain-{}", std::process::id()))
}

fn stub(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Writes the stubs and points the env at them. Runs once, before any test calls the chain.
fn init() -> PathBuf {
    static ONCE: Once = Once::new();
    let cap = cap();
    ONCE.call_once(|| {
        let _ = fs::remove_dir_all(&cap);
        fs::create_dir_all(&cap).unwrap();
        let c = cap.display();
        stub(
            &cap.join("claude"),
            &format!(
                "#!/bin/sh\n\
                 echo \"${{ANTHROPIC_BASE_URL:-unset}}\" > {c}/claude.env\n\
                 input=$(cat)\n\
                 case \"$input\" in\n\
                 *HAIKUDOWN*) exit 1;;\n\
                 *HAIKUSLOW*) sleep 5; exit 1;;\n\
                 esac\n\
                 printf 'receipt: yes\\nmerchant: Haiku Mart\\ntotal: 9.99\\ncategory: other\\n'\n"
            ),
        );
        stub(
            &cap.join("curl"),
            &format!(
                "#!/bin/sh\n\
                 printf '%s\\n' \"$@\" > {c}/curl.args\n\
                 cat > {c}/curl.body\n\
                 if grep -q CURLDOWN {c}/curl.body; then exit 22; fi\n\
                 if grep -q CURLSLOW {c}/curl.body; then sleep 5; exit 22; fi\n\
                 cat {c}/openrouter.json\n"
            ),
        );
        fs::write(cap.join("openrouter.json"), openrouter_body()).unwrap();
        fs::write(cap.join("openrouter_key"), "sk-or-test-key\n").unwrap();
        std::env::set_var("DISCORD_INBOX_CLAUDE", cap.join("claude"));
        std::env::set_var("DISCORD_INBOX_CURL", cap.join("curl"));
        std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY", cap.join("openrouter_key"));
        std::env::set_var("DISCORD_INBOX_DIR", cap.join("inbox"));
        std::env::set_var("ANTHROPIC_BASE_URL", "http://127.0.0.1:8787");
    });
    cap
}

fn ent(text: &str, ty: &str, confidence: f64) -> Value {
    json!({"text": text, "type": ty, "confidence": confidence})
}

/// A chat.completion whose content is the ScaleDown result JSON, as OpenRouter returns it.
fn body_with(entities: Vec<Value>) -> String {
    let inner = json!({"entities": entities}).to_string();
    json!({"id": "x", "object": "chat.completion", "choices": [{"index": 0, "message": {"role": "assistant", "content": inner}}]}).to_string()
}

fn openrouter_body() -> String {
    body_with(vec![
        ent("Hua Lian", "merchant", 0.93),
        ent("$123", "total", 0.88),
        ent("yes", "receipt", 0.97),
        ent("groceries", "category", 0.81),
    ])
}

fn names(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn model_reads_env() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("DISCORD_INBOX_INSIGHT", "");
    assert_eq!(insight::model(), Model::Off);
    std::env::set_var("DISCORD_INBOX_INSIGHT", "/bin/echo hi");
    assert_eq!(insight::model(), Model::Cmd(vec!["/bin/echo".into(), "hi".into()]));
    std::env::remove_var("DISCORD_INBOX_INSIGHT");
    assert_eq!(insight::model(), Model::Chain);
}

#[test]
fn scaledown_request_is_json_with_zdr_and_entities() {
    let inputs = [
        "TRADER JOE'S\nTOTAL 12.34".to_string(),
        "He said \"hi\" \\ back\nline2\ttab".to_string(),
        "蝦條 32\n極佳 特級魚豆腐 45\n合計 NT$123".to_string(),
        "x ".repeat(2500),
    ];
    for input in &inputs {
        let v: Value = serde_json::from_str(&insight::scaledown_request(input)).expect("valid JSON");
        assert_eq!(v["model"], "scaledown/extract");
        assert_eq!(v["provider"]["zdr"], true);
        let want: String = input.chars().take(4000).collect();
        assert_eq!(v["messages"][0]["content"].as_str().unwrap(), want);
        let ents = v["provider"]["options"]["scaledown"]["entities"].as_object().unwrap();
        let mut keys: Vec<&String> = ents.keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["category", "merchant", "receipt", "total"]);
        assert_eq!(names(&ents["receipt"]["labels"]), vec!["yes", "no"]);
        assert_eq!(
            names(&ents["category"]["labels"]),
            vec!["groceries", "dining", "games", "household", "transport", "school", "other"]
        );
    }
}

#[test]
fn parse_scaledown_maps_entities() {
    let parse = |b: String| insight::parse_scaledown(&b);

    let a = parse(body_with(vec![
        ent("Hua Lian", "merchant", 0.93),
        ent("$123", "total", 0.88),
        ent("yes", "receipt", 0.97),
        ent("groceries", "category", 0.81),
    ]))
    .unwrap();
    assert_eq!(
        a,
        Insight { receipt: true, merchant: "Hua Lian".into(), total: "123".into(), category: "groceries".into() }
    );

    let b = parse(body_with(vec![ent("A", "merchant", 0.4), ent("Hua Lian", "merchant", 0.9)])).unwrap();
    assert_eq!(b.merchant, "Hua Lian");

    let c = parse(body_with(vec![ent("X", "merchant", 0.2)])).unwrap();
    assert_eq!(c.merchant, "unknown");

    let d = parse(body_with(vec![ent("$5", "total", 0.9), ent("no", "receipt", 0.9)])).unwrap();
    assert!(!d.receipt);

    let e = parse(body_with(vec![ent("$8.00", "total", 0.8)])).unwrap();
    assert!(e.receipt);

    let f = parse(body_with(vec![])).unwrap();
    assert!(!f.receipt);

    let g = json!({"choices": [{"message": {"content": "Sorry, I can't help"}}]}).to_string();
    assert_eq!(insight::parse_scaledown(&g), None);

    let h = json!({"error": {"code": 400, "message": "bad"}}).to_string();
    assert_eq!(insight::parse_scaledown(&h), None);

    let i = parse(body_with(vec![])).unwrap();
    assert_eq!(
        i,
        Insight { receipt: false, merchant: "unknown".into(), total: "unknown".into(), category: "unknown".into() }
    );
}

#[tokio::test]
async fn chain_end_to_end() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cap = init();
    std::env::remove_var("DISCORD_INBOX_INSIGHT");
    let s = Duration::from_secs(1);
    let total = Duration::from_secs(3);
    let key = cap.join("openrouter_key");

    // (a) ScaleDown answers with the key read by curl, never in argv or body; Haiku is never spawned.
    let _ = fs::remove_file(cap.join("claude.env"));
    let _ = fs::remove_file(cap.join("curl.args"));
    let a = insight::ask_chain("TOTAL 123 HAIKUDOWN", s, total).expect("scaledown insight");
    assert_eq!(a, Insight { receipt: true, merchant: "Hua Lian".into(), total: "123".into(), category: "groceries".into() });
    assert!(!cap.join("claude.env").exists(), "haiku spawned while scaledown answered");
    let args = fs::read_to_string(cap.join("curl.args")).unwrap();
    let arg_lines: Vec<&str> = args.lines().collect();
    assert!(arg_lines.contains(&"--variable"), "{args}");
    assert!(arg_lines.contains(&format!("KEY@{}", key.display()).as_str()), "{args}");
    assert!(arg_lines.contains(&"--expand-header"), "{args}");
    assert!(arg_lines.contains(&"Authorization: Bearer {{KEY:trim}}"), "{args}");
    assert!(arg_lines.contains(&"--max-time"), "{args}");
    assert!(arg_lines.contains(&"--data-binary"), "{args}");
    assert!(arg_lines.contains(&"@-"), "{args}");
    assert_eq!(arg_lines.last().copied(), Some(OPENROUTER), "{args}");
    assert!(!args.contains("sk-or-test-key"), "{args}");
    let body = fs::read_to_string(cap.join("curl.body")).unwrap();
    assert!(!body.contains("sk-or-test-key"), "{body}");
    let v: Value = serde_json::from_str(&body).expect("body parses");
    assert_eq!(v["provider"]["zdr"], true);
    assert_eq!(v["messages"][0]["content"], "TOTAL 123 HAIKUDOWN");

    // (b) ScaleDown fails; Haiku answers and sees no router.
    let _ = fs::remove_file(cap.join("claude.env"));
    let b = insight::ask_chain("TOTAL 123 CURLDOWN", s, total).expect("haiku insight");
    assert_eq!(b.merchant, "Haiku Mart");
    assert_eq!(fs::read_to_string(cap.join("claude.env")).unwrap().trim(), "unset");

    // (c) ScaleDown fails and Haiku fails too: nothing.
    assert_eq!(insight::ask_chain("HAIKUDOWN CURLDOWN", s, total), None);

    // (d) ScaleDown is cut at total minus Haiku's cap (2 s of the 3 s total), and Haiku still
    // answers within the total.
    let started = Instant::now();
    let d = insight::ask_chain("CURLSLOW", s, total).expect("haiku after scaledown timeout");
    assert_eq!(d.merchant, "Haiku Mart");
    assert!(started.elapsed() < total, "took {:?}", started.elapsed());

    // (e) No key file: no curl process at all, and Haiku answers.
    std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY", cap.join("no-such-key"));
    let _ = fs::remove_file(cap.join("curl.args"));
    assert_eq!(insight::ask_chain("HAIKUDOWN", s, total), None);
    assert!(!cap.join("curl.args").exists());
    assert_eq!(insight::ask_chain("TRADER JOES", s, total).expect("haiku, no key").merchant, "Haiku Mart");
    assert!(!cap.join("curl.args").exists());
    std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY", &key);

    // (f) Through the real writer: the card number is masked before either model sees it.
    std::env::remove_var("DISCORD_INBOX_INSIGHT");
    let _ = fs::remove_file(cap.join("curl.body"));
    let msg = Incoming {
        msg_id: 1,
        author_id: 1,
        received: "2026-10-14T14:03:22Z".into(),
        content: String::new(),
        attachments: vec![],
        ocr: vec![(0, "VISA 4111 1111 1111 1111 TOTAL 123 HAIKUDOWN".into())],
    };
    let reply = insight::compose(&msg).await.expect("a reply");
    let sent: Value = serde_json::from_str(&fs::read_to_string(cap.join("curl.body")).unwrap()).unwrap();
    let sent = sent["messages"][0]["content"].as_str().unwrap();
    assert!(!sent.contains("4111"), "{sent}");
    assert!(sent.ends_with("1111 TOTAL 123 HAIKUDOWN"), "{sent}");
    assert!(reply.contains("Receipt: Hua Lian"), "{reply}");
    assert!(!reply.contains("4111"), "{reply}");
}

/// The order is ScaleDown first, then Haiku. Both stubs succeed in (1), so ScaleDown's merchant
/// is the proof it ran first, and no `claude` process may start.
#[test]
fn scaledown_first_then_haiku() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cap = init();
    std::env::remove_var("DISCORD_INBOX_INSIGHT");
    let s = Duration::from_secs(1);
    let total = Duration::from_secs(3);
    let key = cap.join("openrouter_key");

    // (1) Key present, both succeed: ScaleDown answers and Haiku is never spawned.
    std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY", &key);
    let _ = fs::remove_file(cap.join("claude.env"));
    let got = insight::ask_chain("TRADER JOES", s, total).expect("scaledown insight");
    assert_eq!(got.merchant, "Hua Lian");
    assert!(!cap.join("claude.env").exists(), "haiku was spawned while scaledown answered");

    // (2) No key file: no curl process, and Haiku answers.
    std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY", cap.join("no-such-key"));
    let _ = fs::remove_file(cap.join("curl.args"));
    let got = insight::ask_chain("TRADER JOES", s, total).expect("haiku insight");
    assert_eq!(got.merchant, "Haiku Mart");
    assert!(!cap.join("curl.args").exists());

    // (3) Key present, ScaleDown fails: Haiku answers.
    std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY", &key);
    let got = insight::ask_chain("TOTAL 123 CURLDOWN", s, total).expect("haiku after scaledown failed");
    assert_eq!(got.merchant, "Haiku Mart");
}

/// The key file counts only when it has a non-empty line. An empty or blank key file is no key:
/// no curl process, and Haiku answers.
#[test]
fn empty_key_file_means_no_scaledown() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cap = init();
    std::env::remove_var("DISCORD_INBOX_INSIGHT");
    let s = Duration::from_secs(1);
    let total = Duration::from_secs(3);
    let empty = cap.join("empty_key");
    fs::write(&empty, "  \n").unwrap();
    std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY", &empty);
    let _ = fs::remove_file(cap.join("curl.args"));
    let got = insight::ask_chain("TRADER JOES", s, total).expect("haiku insight");
    assert_eq!(got.merchant, "Haiku Mart");
    assert!(!cap.join("curl.args").exists(), "curl ran with an empty key file");
    std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY", cap.join("openrouter_key"));
}

/// Runs the built Vision helper on a committed synthetic receipt image (image on stdin, text on stdout).
fn vision_reads(fixture: &str) -> String {
    use std::io::Write;
    let img = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture)).unwrap();
    let mut child = std::process::Command::new(env!("DISCORD_INBOX_VISION"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the Vision helper did not build or start");
    child.stdin.take().unwrap().write_all(&img).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Review finding 1 and 2: the built helper reads a synthetic Traditional-Chinese receipt (store name,
/// 總計 line, amount), and a synthetic English receipt still gives its TOTAL line and amount.
#[test]
fn vision_helper_reads_chinese() {
    let zh = vision_reads("receipt_zh.png");
    assert!(zh.contains("華聯"), "store name lost: {zh}");
    assert!(zh.contains("總計"), "總計 line lost: {zh}");
    assert!(zh.contains("9.70"), "amount lost: {zh}");
}

#[test]
fn vision_helper_reads_english_receipt() {
    let en = vision_reads("receipt_en.png");
    assert!(en.contains("TOTAL"), "TOTAL line lost: {en}");
    assert!(en.contains("9.70"), "amount lost: {en}");
}

/// Review finding 1: a photo whose OCR text is blank must not reach either model.
#[tokio::test]
async fn blank_photo_makes_no_model_call() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cap = init();
    std::env::remove_var("DISCORD_INBOX_INSIGHT");
    for blank in ["", "  \n\t "] {
        let _ = fs::remove_file(cap.join("claude.env"));
        let _ = fs::remove_file(cap.join("curl.args"));
        let msg = Incoming {
            msg_id: 2,
            author_id: 1,
            received: "2026-10-14T14:03:22Z".into(),
            content: String::new(),
            attachments: vec![],
            ocr: vec![(0, blank.into())],
        };
        let _ = insight::compose(&msg).await;
        assert!(!cap.join("claude.env").exists(), "blank OCR {blank:?} reached claude");
        assert!(!cap.join("curl.args").exists(), "blank OCR {blank:?} reached curl");
    }
}

/// Review finding 5: a quoted empty `DISCORD_INBOX_INSIGHT` means off, not a command named `""`.
#[test]
fn quoted_empty_insight_is_off() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for v in ["''", "\"\"", "  "] {
        std::env::set_var("DISCORD_INBOX_INSIGHT", v);
        assert_eq!(insight::model(), Model::Off, "DISCORD_INBOX_INSIGHT={v:?}");
    }
    std::env::remove_var("DISCORD_INBOX_INSIGHT");
}
