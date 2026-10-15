//! The model transport: reply extraction, the key file, and the offline fallback. No network.
use discord_inbox::chat::{self, Chat, NoChat, OpenRouter};
use std::fs;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("di-chat-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn extract_content_reads_openrouter_body() {
    let body = r#"{"id":"gen-1","choices":[{"index":0,"message":{"role":"assistant","content":"  FOLDER: Citadel/Ideas\nTITLE: Solar fan\n  "},"finish_reason":"stop"}],"usage":{"total_tokens":10}}"#;
    assert_eq!(
        chat::extract_content(body).unwrap(),
        "FOLDER: Citadel/Ideas\nTITLE: Solar fan"
    );
}

#[test]
fn extract_content_rejects_empty_missing_nonstring_and_nonjson() {
    for body in [
        r#"{"choices":[]}"#,
        r#"{"choices":[{"message":{"content":""}}]}"#,
        r#"{"choices":[{"message":{"content":"   "}}]}"#,
        r#"{"choices":[{"message":{"content":3}}]}"#,
        r#"{"error":{"message":"no endpoints"}}"#,
        "<html>502</html>",
        "",
    ] {
        assert!(chat::extract_content(body).is_err(), "accepted {body}");
    }
}

#[test]
fn nochat_ask_is_err() {
    assert!(NoChat.ask("system", "user").is_err());
}

#[test]
fn from_key_file_missing_empty_and_trimmed() {
    let d = tmp("key");
    assert!(OpenRouter::from_key_file(&d.join("missing")).is_none());
    fs::write(d.join("empty"), "  \n").unwrap();
    assert!(OpenRouter::from_key_file(&d.join("empty")).is_none());
    fs::write(d.join("key"), "  sk-or-test \n").unwrap();
    assert!(OpenRouter::from_key_file(&d.join("key")).is_some());
    // The env var overrides the default location.
    std::env::set_var("DISCORD_INBOX_OPENROUTER_KEY_FILE", d.join("key"));
    assert_eq!(chat::key_path(), d.join("key"));
    std::env::remove_var("DISCORD_INBOX_OPENROUTER_KEY_FILE");
}

/// The request body pins the model and the zero-data-retention flag. Offline: the body is built, never sent.
#[test]
fn request_body_pins_model_and_zdr() {
    let body = chat::request_body("system text", "user text");
    assert_eq!(body["provider"]["zdr"], serde_json::Value::Bool(true), "{body}");
    assert_eq!(body["model"], "xiaomi/mimo-v2.6-flash", "{body}");
    assert_eq!(body["max_tokens"], 500, "{body}");
    assert_eq!(body["messages"][0]["role"], "system", "{body}");
    assert_eq!(body["messages"][1]["content"], "user text", "{body}");
}
