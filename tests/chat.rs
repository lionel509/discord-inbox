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
    assert_eq!(body["model"], "xiaomi/mimo-v2.6-pro", "{body}");
    assert_eq!(body["max_tokens"], 500, "{body}");
    assert_eq!(body["messages"][0]["role"], "system", "{body}");
    assert_eq!(body["messages"][1]["content"], "user text", "{body}");
}

// ---- #49: function calling on the same transport ----

use discord_inbox::chat::{Reply, Turn, Usage};
use serde_json::json;

#[test]
fn extract_reply_parses_tool_calls_and_usage() {
    let body = r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"search_notes","arguments":"{\"query\":\"pantry cod\"}"}}]}}],"usage":{"prompt_tokens":120,"completion_tokens":14}}"#;
    let r = chat::extract_reply(body).unwrap();
    assert_eq!(r.usage, Usage { prompt: 120, completion: 14 });
    match r.turn {
        Turn::Calls(cs) => {
            assert_eq!(cs.len(), 1);
            assert_eq!(cs[0].id, "c1");
            assert_eq!(cs[0].name, "search_notes");
            assert_eq!(cs[0].arguments, r#"{"query":"pantry cod"}"#);
        }
        _ => panic!("expected tool calls"),
    }
}

#[test]
fn extract_reply_text_when_no_tool_calls() {
    let body = r#"{"choices":[{"message":{"role":"assistant","content":" Fish tonight. "}}],"usage":{"prompt_tokens":5,"completion_tokens":3}}"#;
    let r = chat::extract_reply(body).unwrap();
    assert!(matches!(r.turn, Turn::Text(ref t) if t == "Fish tonight."));
}

#[test]
fn extract_reply_errs_on_null_content_without_calls() {
    let body = r#"{"choices":[{"message":{"role":"assistant","content":null}}]}"#;
    assert!(chat::extract_reply(body).is_err());
}

#[test]
fn tools_body_sets_tools_zdr_and_tool_choice_auto() {
    let msgs = vec![json!({"role": "system", "content": "sys"}), json!({"role": "user", "content": "q"})];
    let tools = vec![json!({"type": "function", "function": {"name": "list_folder"}})];
    let b = chat::tools_body(&msgs, &tools, true);
    assert_eq!(b["model"], chat::MODEL);
    assert_eq!(b["tools"], json!(tools));
    assert_eq!(b["tool_choice"], "auto");
    assert_eq!(b["provider"]["zdr"], true);
}

#[test]
fn tools_body_tool_choice_none_and_no_tools_key_when_empty() {
    let msgs = vec![json!({"role": "user", "content": "q"})];
    let b = chat::tools_body(&msgs, &[], false);
    assert!(b.get("tool_choice").is_none());
    assert!(b.get("tools").is_none());
}

#[test]
fn tools_body_redacts_every_content_string_including_tool_role() {
    let msgs = vec![json!({"role": "tool", "tool_call_id": "c1", "content": "card 4111 1111 1111 1111"})];
    let b = chat::tools_body(&msgs, &[], true);
    let s = b.to_string();
    assert!(!s.contains("4111 1111"), "{s}");
}

struct OnlyAsk;

impl Chat for OnlyAsk {
    fn ask(&self, system: &str, user: &str) -> Result<String, String> {
        Ok(format!("{system}|{user}"))
    }
}

#[test]
fn default_ask_tools_uses_last_user_message_and_never_calls() {
    let msgs = vec![
        json!({"role": "system", "content": "S"}),
        json!({"role": "user", "content": "first"}),
        json!({"role": "assistant", "content": null}),
        json!({"role": "user", "content": "last"}),
    ];
    let r = OnlyAsk.ask_tools(&msgs, &[], true).unwrap();
    assert!(matches!(r.turn, Turn::Text(ref t) if t == "S|last"));
    assert_eq!(r.usage, Usage::default());
}

#[test]
fn nochat_ask_tools_errs() {
    assert!(NoChat.ask_tools(&[], &[], true).is_err());
}
