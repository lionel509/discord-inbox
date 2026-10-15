//! The agentic lookup loop (#49) with a scripted model. Temp dirs and a made-up `Private` folder only.
//! The live demo at the bottom is `#[ignore]`: it needs the real OpenRouter key.
use discord_inbox::agent::{answer, LIMIT_NOTE, MAX_CALLS, BUDGET_CHARS};
use discord_inbox::chat::{self, Chat, OpenRouter, Reply, ToolCall, Turn, Usage};
use discord_inbox::dump;
use discord_inbox::tools::{Lookup, ROOTS};
use discord_inbox::vaults::Vaults;
use serde_json::Value;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Replies come off a queue; every request's messages and `allow_tools` flag are recorded.
struct Script {
    replies: Mutex<VecDeque<Result<Reply, String>>>,
    seen: Mutex<Vec<(Vec<Value>, bool)>>,
}

impl Script {
    fn new(replies: Vec<Result<Reply, String>>) -> Script {
        Script { replies: Mutex::new(replies.into()), seen: Mutex::new(Vec::new()) }
    }
}

impl Chat for Script {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Err("ask is not used by the agent".into())
    }
    fn ask_tools(&self, messages: &[Value], _tools: &[Value], allow_tools: bool) -> Result<Reply, String> {
        self.seen.lock().unwrap().push((messages.to_vec(), allow_tools));
        self.replies.lock().unwrap().pop_front().unwrap_or_else(|| Err("script empty".into()))
    }
}

fn put(p: &Path, body: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn lookup(name: &str) -> (Lookup, PathBuf) {
    let base = std::env::temp_dir().join(format!("di-agent-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let root = base.join("vault");
    for r in ROOTS.iter().filter(|r| **r != "Bridgewater") {
        fs::create_dir_all(root.join(r)).unwrap();
    }
    put(&root.join("Fidelity/Pantry/Pantry.md"), "cod fillets, soy sauce, gochujang, rice\n");
    put(&root.join("Private/secret.md"), "SECRET-PRIVATE-TOKEN");
    put(&root.join("Fidelity/Pantry/card.md"), "card 4111 1111 1111 1111 for groceries");
    put(&root.join("Fidelity/Big.md"), &"cod ".repeat(BUDGET_CHARS));
    let inbox = base.join("Inbox");
    (Lookup::new(root.clone(), inbox), root)
}

fn calls(id: &str, name: &str, args: &str) -> Reply {
    Reply {
        turn: Turn::Calls(vec![ToolCall { id: id.into(), name: name.into(), arguments: args.into() }]),
        usage: Usage { prompt: 10, completion: 2 },
    }
}

fn text(t: &str) -> Reply {
    Reply { turn: Turn::Text(t.into()), usage: Usage { prompt: 20, completion: 5 } }
}

fn tool_msgs(messages: &[Value]) -> Vec<Value> {
    messages.iter().filter(|m| m["role"] == "tool").cloned().collect()
}

const Q: &str = "with the ingredients in my pantry what can i do with my cod";

#[test]
fn loop_searches_reads_pantry_then_answers() {
    let (lk, _) = lookup("loop");
    let chat = Script::new(vec![
        Ok(calls("c1", "search_notes", r#"{"query":"pantry cod"}"#)),
        Ok(calls("c2", "read_note", r#"{"path":"Fidelity/Pantry/Pantry.md"}"#)),
        Ok(text("Cod fillets with soy sauce and rice.")),
    ]);
    let a = answer(&chat, &lk, Q).unwrap();
    assert_eq!(a.text, "Cod fillets with soy sauce and rice.");
    assert_eq!(a.calls, 2);
    assert_eq!(a.limited, None);
    assert_eq!(a.usage, Usage { prompt: 40, completion: 9 });
    let seen = chat.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 3);
    let tools = tool_msgs(&seen[2].0);
    assert_eq!(tools.len(), 2);
    assert!(tools[0]["content"].as_str().unwrap().contains("Fidelity/Pantry/Pantry.md"));
    assert!(tools[1]["content"].as_str().unwrap().contains("cod fillets"));
}

#[test]
fn assistant_tool_call_message_echoes_id_name_arguments() {
    let (lk, _) = lookup("echo");
    let chat = Script::new(vec![Ok(calls("c9", "list_folder", r#"{"path":"Fidelity"}"#)), Ok(text("ok"))]);
    answer(&chat, &lk, Q).unwrap();
    let seen = chat.seen.lock().unwrap().clone();
    let asst = seen[1].0.iter().find(|m| m["role"] == "assistant").expect("assistant message");
    let tc = &asst["tool_calls"][0];
    assert_eq!(tc["id"], "c9");
    assert_eq!(tc["type"], "function");
    assert_eq!(tc["function"]["name"], "list_folder");
    assert_eq!(tc["function"]["arguments"], r#"{"path":"Fidelity"}"#);
    assert_eq!(tool_msgs(&seen[1].0)[0]["tool_call_id"], "c9");
}

#[test]
fn unknown_tool_and_bad_json_go_back_to_model_as_error_text() {
    let (lk, _) = lookup("errors");
    let chat = Script::new(vec![
        Ok(calls("a", "delete_note", "{}")),
        Ok(calls("b", "search_notes", "not json")),
        Ok(text("sorry")),
    ]);
    let a = answer(&chat, &lk, Q).unwrap();
    assert_eq!(a.calls, 2);
    let seen = chat.seen.lock().unwrap().clone();
    let tools = tool_msgs(&seen[2].0);
    assert!(tools[0]["content"].as_str().unwrap().starts_with("error: "));
    assert!(tools[1]["content"].as_str().unwrap().starts_with("error: "));
}

#[test]
fn stops_at_max_calls_then_final_call_has_tools_off() {
    let (lk, _) = lookup("max");
    let mut q: Vec<Result<Reply, String>> =
        (0..MAX_CALLS).map(|i| Ok(calls(&format!("c{i}"), "list_folder", r#"{"path":""}"#))).collect();
    q.push(Ok(text("I couldn't check everything, but here is what I have.")));
    let chat = Script::new(q);
    let a = answer(&chat, &lk, Q).unwrap();
    assert_eq!(a.calls, MAX_CALLS);
    assert_eq!(a.limited, Some("lookups"));
    let seen = chat.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), MAX_CALLS + 1);
    let last = seen.last().unwrap();
    assert!(!last.1, "the final call must have tools off");
    assert_eq!(tool_msgs(&last.0).len(), MAX_CALLS);
    assert_eq!(last.0.last().unwrap()["content"], LIMIT_NOTE);
}

#[test]
fn two_calls_in_one_round_count_twice() {
    let (lk, _) = lookup("two");
    let two = Reply {
        turn: Turn::Calls(vec![
            ToolCall { id: "a".into(), name: "list_folder".into(), arguments: r#"{"path":"Fidelity"}"#.into() },
            ToolCall { id: "b".into(), name: "search_notes".into(), arguments: r#"{"query":"pantry"}"#.into() },
        ]),
        usage: Usage::default(),
    };
    let chat = Script::new(vec![Ok(two), Ok(text("done"))]);
    let a = answer(&chat, &lk, Q).unwrap();
    assert_eq!(a.calls, 2);
    let seen = chat.seen.lock().unwrap().clone();
    assert_eq!(tool_msgs(&seen[1].0).len(), 2);
}

#[test]
fn budget_cut_marks_result_and_sets_limited_budget() {
    let (lk, _) = lookup("budget");
    // Each read of the big note is about 6K chars, so the fifth read exhausts the 30K budget.
    let mut q: Vec<Result<Reply, String>> =
        (0..5).map(|i| Ok(calls(&format!("r{i}"), "read_note", r#"{"path":"Fidelity/Big.md"}"#))).collect();
    q.push(Ok(text("Cod, mostly.")));
    let chat = Script::new(q);
    let a = answer(&chat, &lk, Q).unwrap();
    assert_eq!(a.limited, Some("budget"));
    let seen = chat.seen.lock().unwrap().clone();
    let last_tools = tool_msgs(&seen.last().unwrap().0);
    assert!(last_tools.iter().any(|m| m["content"].as_str().unwrap().contains("[cut: budget]")));
    let total: usize = last_tools.iter().map(|m| m["content"].as_str().unwrap().chars().count()).sum();
    assert!(total <= BUDGET_CHARS, "{total}");
    assert!(!seen.last().unwrap().1);
}

#[test]
fn calls_after_limit_is_err() {
    let (lk, _) = lookup("after");
    let mut q: Vec<Result<Reply, String>> =
        (0..MAX_CALLS).map(|i| Ok(calls(&format!("c{i}"), "list_folder", r#"{"path":""}"#))).collect();
    q.push(Ok(calls("x", "list_folder", "{}")));
    let chat = Script::new(q);
    assert!(answer(&chat, &lk, Q).is_err());
}

#[test]
fn transport_err_propagates() {
    let (lk, _) = lookup("transport");
    let chat = Script::new(vec![Err("http 404".into())]);
    assert_eq!(answer(&chat, &lk, Q).err().as_deref(), Some("http 404"));
}

#[test]
fn read_note_result_in_messages_is_redacted() {
    let (lk, _) = lookup("redacted");
    let chat = Script::new(vec![
        Ok(calls("c1", "read_note", r#"{"path":"Fidelity/Pantry/card.md"}"#)),
        Ok(text("Groceries.")),
    ]);
    answer(&chat, &lk, Q).unwrap();
    let seen = chat.seen.lock().unwrap().clone();
    let content = tool_msgs(&seen[1].0)[0]["content"].as_str().unwrap().to_string();
    assert!(!content.contains("4111 1111"), "{content}");
}

/// Demo on the real model: `cargo test --test agent -- --ignored --nocapture live_pantry_question`.
#[test]
#[ignore]
fn live_pantry_question() {
    let key = OpenRouter::from_key_file(&chat::key_path()).expect("no OpenRouter key file");
    let lk = Lookup::new(Vaults::default_root(), dump::inbox_dir());
    let a = answer(&key, &lk, Q).unwrap();
    println!("{}\n[calls {} tokens {}+{} limit {:?}]", a.text, a.calls, a.usage.prompt, a.usage.completion, a.limited);
}

// Regression (PR #51 review round 1): parallel calls in one turn must respect MAX_CALLS.
#[test]
fn one_round_of_many_calls_cannot_exceed_max_calls() {
    let (lk, _) = lookup("burst");
    let burst = Reply {
        turn: Turn::Calls(
            (0..10)
                .map(|i| ToolCall { id: format!("c{i}"), name: "search_notes".into(), arguments: r#"{"query":"pantry"}"#.into() })
                .collect(),
        ),
        usage: Usage::default(),
    };
    let chat = Script::new(vec![Ok(burst), Ok(text("done"))]);
    let a = answer(&chat, &lk, Q).unwrap();
    assert_eq!(a.calls, MAX_CALLS);
    assert_eq!(a.limited, Some("lookups"));
    let seen = chat.seen.lock().unwrap().clone();
    let tools = tool_msgs(&seen[1].0);
    assert_eq!(tools.len(), 10, "every tool_call_id still gets a reply");
    assert_eq!(tools[9]["content"], "error: lookup limit reached");
}
