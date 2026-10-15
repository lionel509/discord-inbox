//! Model transport for the chat reply (#31). The model is called through OpenRouter with
//! zero data retention on; the key lives in a file, never in the environment or the logs.
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MODEL: &str = "xiaomi/mimo-v2.6-flash";
pub const TIMEOUT: Duration = Duration::from_secs(30);
const URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// One tool call the model asked for. `arguments` is the raw JSON string as OpenRouter sent it.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// What the model answered: plain text, or a list of tool calls to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Turn {
    Text(String),
    Calls(Vec<ToolCall>),
}

/// Token counts from the OpenRouter `usage` block; zero when absent.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Usage {
    pub prompt: u64,
    pub completion: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub turn: Turn,
    pub usage: Usage,
}

/// Object-safe and synchronous on purpose: callers run it under `spawn_blocking`.
pub trait Chat: Send + Sync {
    fn ask(&self, system: &str, user: &str) -> Result<String, String>;

    /// Function calling. The default never calls a tool: it sends the first system message and
    /// the last user message through `ask`, so every one-shot fake keeps working.
    fn ask_tools(
        &self,
        messages: &[serde_json::Value],
        _tools: &[serde_json::Value],
        _allow_tools: bool,
    ) -> Result<Reply, String> {
        let content = |role: &str| -> String {
            messages
                .iter()
                .rev()
                .find(|m| m["role"] == role)
                .and_then(|m| m["content"].as_str())
                .unwrap_or("")
                .to_string()
        };
        let system = content("system");
        let user = content("user");
        self.ask(&system, &user).map(|t| Reply {
            turn: Turn::Text(t),
            usage: Usage::default(),
        })
    }
}

/// The offline fallback: every ask fails, so the caller keeps today's ✅ path.
pub struct NoChat;

impl Chat for NoChat {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Err("chat off".into())
    }
}

/// Where the OpenRouter key lives: `$DISCORD_INBOX_OPENROUTER_KEY_FILE` or the default file.
pub fn key_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DISCORD_INBOX_OPENROUTER_KEY_FILE") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/discord-inbox/openrouter_key")
}

/// The key is kept private to this struct: it is never formatted, logged or returned.
pub struct OpenRouter {
    key: String,
    client: reqwest::blocking::Client,
}

impl OpenRouter {
    pub fn new(key: String) -> OpenRouter {
        let client = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        OpenRouter { key, client }
    }

    /// `None` when the file is missing, unreadable or holds only whitespace.
    pub fn from_key_file(path: &Path) -> Option<OpenRouter> {
        let raw = std::fs::read_to_string(path).ok()?;
        let key = raw.trim();
        if key.is_empty() {
            return None;
        }
        Some(OpenRouter::new(key.to_string()))
    }
}

/// Pulls `choices[0].message.content` out of an OpenRouter body; `Err` when absent or blank.
pub fn extract_content(body: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("bad json: {e}"))?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "no content".to_string())?
        .trim();
    if text.is_empty() {
        return Err("empty content".into());
    }
    Ok(text.to_string())
}

/// Parses a function-calling reply. Tool calls win over text; `Err` when there is neither.
pub fn extract_reply(body: &str) -> Result<Reply, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("bad json: {e}"))?;
    let usage = Usage {
        prompt: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        completion: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
    };
    let msg = &v["choices"][0]["message"];
    if let Some(calls) = msg["tool_calls"].as_array().filter(|c| !c.is_empty()) {
        let mut out = Vec::with_capacity(calls.len());
        for c in calls {
            let name = c["function"]["name"]
                .as_str()
                .ok_or_else(|| "bad tool call".to_string())?;
            out.push(ToolCall {
                id: c["id"].as_str().unwrap_or("").to_string(),
                name: name.to_string(),
                arguments: c["function"]["arguments"].as_str().unwrap_or("{}").to_string(),
            });
        }
        return Ok(Reply { turn: Turn::Calls(out), usage });
    }
    let text = msg["content"]
        .as_str()
        .ok_or_else(|| "no content".to_string())?
        .trim();
    if text.is_empty() {
        return Err("empty content".into());
    }
    Ok(Reply { turn: Turn::Text(text.to_string()), usage })
}

/// The OpenRouter request. Zero data retention is on (`provider.zdr`), so a provider that cannot promise it 404s.
pub fn request_body(system: &str, user: &str) -> serde_json::Value {
    serde_json::json!({
        "model": MODEL,
        "messages": [
            {"role": "system", "content": crate::redact::sensitive(system)},
            {"role": "user", "content": crate::redact::sensitive(user)},
        ],
        "provider": {"zdr": true},
        "max_tokens": 500,
        "temperature": 0.3,
    })
}

/// The function-calling request: messages are redacted (tool results too), `tool_choice` is `none`
/// on the final call so the model must answer, and `tools` is left out when there are none.
pub fn tools_body(
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    allow_tools: bool,
) -> serde_json::Value {
    let msgs: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            let mut m = m.clone();
            if let Some(c) = m["content"].as_str().map(str::to_string) {
                m["content"] = serde_json::Value::String(crate::redact::sensitive(&c));
            }
            m
        })
        .collect();
    let mut body = serde_json::json!({
        "model": MODEL,
        "messages": msgs,
        "provider": {"zdr": true},
        "max_tokens": 500,
        "temperature": 0.3,
    });
    if !tools.is_empty() {
        body["tool_choice"] = serde_json::json!(if allow_tools { "auto" } else { "none" });
        body["tools"] = serde_json::Value::Array(tools.to_vec());
    }
    body
}

impl Chat for OpenRouter {
    fn ask(&self, system: &str, user: &str) -> Result<String, String> {
        let body = request_body(system, user);
        let resp = self
            .client
            .post(URL)
            .header("Authorization", format!("Bearer {}", self.key))
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        // A non-2xx status (a 404 from the ZDR guardrail included) is a failure, never a reply.
        if !status.is_success() {
            return Err(format!("http {}", status.as_u16()));
        }
        let text = resp.text().map_err(|e| e.to_string())?;
        extract_content(&text)
    }

    fn ask_tools(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        allow_tools: bool,
    ) -> Result<Reply, String> {
        let body = tools_body(messages, tools, allow_tools);
        let resp = self
            .client
            .post(URL)
            .header("Authorization", format!("Bearer {}", self.key))
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("http {}", status.as_u16()));
        }
        let text = resp.text().map_err(|e| e.to_string())?;
        extract_reply(&text)
    }
}
