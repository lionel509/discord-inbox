//! Model transport for the chat reply (#31). The model is called through OpenRouter with
//! zero data retention on; the key lives in a file, never in the environment or the logs.
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MODEL: &str = "xiaomi/mimo-v2.6-flash";
pub const TIMEOUT: Duration = Duration::from_secs(30);
const URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// Object-safe and synchronous on purpose: callers run it under `spawn_blocking`.
pub trait Chat: Send + Sync {
    fn ask(&self, system: &str, user: &str) -> Result<String, String>;
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

/// The OpenRouter request. Zero data retention is on (`provider.zdr`), so a provider that cannot promise it 404s.
pub fn request_body(system: &str, user: &str) -> serde_json::Value {
    serde_json::json!({
        "model": MODEL,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "provider": {"zdr": true},
        "max_tokens": 500,
        "temperature": 0.3,
    })
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
}
