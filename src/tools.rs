//! The read-only lookup tools (#49): roots, search, read, list and the privacy invariant. No network.
//!
//! Every path is canonicalized under an allowed root. Hidden (`.`-prefixed) components, symlinks,
//! absolute paths and `..` are refused by construction, so nothing outside the roots (the private
//! vault folder included) can reach a tool result. Nothing here writes to disk.
use crate::redact;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// The vault folders the agent may read. `Inbox` is added separately.
pub const ROOTS: [&str; 9] = [
    "BlackRock", "State Street", "Berkshire", "Goldman", "Fidelity", "Citadel", "Mirae", "Bridgewater", "! Todo",
];
pub const SEARCH_HITS: usize = 8;
pub const SNIPPET_CHARS: usize = 200;
pub const READ_CHARS: usize = 6_000;
pub const LIST_MAX: usize = 100;
pub const MAX_FILES: usize = 20_000;
pub const MAX_FILE_BYTES: u64 = 1_000_000;
pub const EXTS: [&str; 3] = ["md", "txt", "csv"];

const REFUSED: &str = "not found or not allowed";

pub struct Lookup {
    root: PathBuf,
    inbox: PathBuf,
}

fn is_real_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).map(|m| m.file_type().is_dir()).unwrap_or(false)
}

fn has_ext(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| EXTS.contains(&e)).unwrap_or(false)
}

/// Collect searchable files under `dir`: hidden names and symlinks skipped, sorted, capped.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if out.len() >= MAX_FILES {
            return;
        }
        let hidden = p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(true);
        if hidden {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&p) else { continue };
        let ft = meta.file_type();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(&p, out);
        } else if ft.is_file() && has_ext(&p) && meta.len() <= MAX_FILE_BYTES {
            out.push(p);
        }
    }
}

/// Read a file as redacted text; `None` when unreadable or not UTF-8.
fn read_text(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|t| redact::sensitive(&t))
}

/// A window of about `SNIPPET_CHARS` around the first word hit, whitespace collapsed to single spaces.
fn snippet(text: &str, lower: &str, words: &[String]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at = words
        .iter()
        .find_map(|w| lower.find(w.as_str()))
        .map(|byte| lower[..byte].chars().count())
        .unwrap_or(0);
    let start = at.saturating_sub(SNIPPET_CHARS / 4).min(chars.len());
    let end = (start + SNIPPET_CHARS).min(chars.len());
    chars[start..end].iter().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Query words this long or longer also match a word one edit away, so a misspelled name still finds its note (#73).
const NEAR_MIN: usize = 5;

/// True when `a` and `b` differ by one inserted, deleted or substituted character.
fn one_edit(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (long, short) = if a.len() >= b.len() { (&a, &b) } else { (&b, &a) };
    if long.len() - short.len() > 1 || a == b {
        return false;
    }
    let i = long.iter().zip(short.iter()).take_while(|(x, y)| x == y).count();
    if long.len() == short.len() {
        long[i + 1..] == short[i + 1..]
    } else {
        long[i + 1..] == short[i..]
    }
}

/// The words of `lower` one edit away from query word `w`; none when `w` is short or already in the text.
fn near_words(lower: &str, w: &str) -> Vec<String> {
    if w.chars().count() < NEAR_MIN || lower.contains(w) {
        return Vec::new();
    }
    lower.split(|c: char| !c.is_alphanumeric()).filter(|t| one_edit(t, w)).map(str::to_string).collect()
}

impl Lookup {
    pub fn new(root: PathBuf, inbox: PathBuf) -> Lookup {
        Lookup { root, inbox }
    }

    /// The readable top-level folders `(name, canonical dir)`: the nine vault roots that are real
    /// folders under the vault (a symlinked root is dropped), then `Inbox`.
    pub fn roots(&self) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        if let Ok(vroot) = self.root.canonicalize() {
            for name in ROOTS {
                let d = self.root.join(name);
                if !is_real_dir(&d) {
                    continue;
                }
                let Ok(c) = d.canonicalize() else { continue };
                if c == vroot.join(name) {
                    out.push((name.to_string(), c));
                }
            }
        }
        if is_real_dir(&self.inbox) {
            if let Ok(c) = self.inbox.canonicalize() {
                out.push(("Inbox".to_string(), c));
            }
        }
        out
    }

    /// Map a display path `"<Vault>/<rest>"` to `(display, canonical path)`, or `None`.
    fn resolve(&self, rel: &str) -> Option<(String, PathBuf)> {
        if Path::new(rel).is_absolute() {
            return None;
        }
        // Models copy the trailing slash from list_folder output; treat "Fidelity/" as "Fidelity".
        let rel = rel.trim_end().trim_end_matches(['/', '\\']).trim_end();
        let parts: Vec<&str> = rel.split(['/', '\\']).collect();
        if parts.iter().any(|p| p.is_empty() || p.starts_with('.')) {
            return None;
        }
        let (first, rest) = parts.split_first()?;
        let (name, base) = self.roots().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(first))?;
        let mut p = base.clone();
        for c in rest {
            p.push(c);
        }
        if fs::symlink_metadata(&p).ok()?.file_type().is_symlink() {
            return None;
        }
        let c = p.canonicalize().ok()?;
        if !c.starts_with(&base) {
            return None;
        }
        let display = std::iter::once(name).chain(rest.iter().map(|s| s.to_string())).collect::<Vec<_>>().join("/");
        Some((display, c))
    }

    pub fn search_notes(&self, query: &str, folder: Option<&str>) -> Result<String, String> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() >= 2)
            .take(8)
            .collect();
        if words.is_empty() {
            return Err("give me a word or two to search for".into());
        }
        let scope: Vec<(String, PathBuf)> = match folder {
            Some(f) => match self.resolve(f) {
                Some((disp, c)) if c.is_dir() => vec![(disp, c)],
                _ => return Err("folder not found or not allowed".into()),
            },
            None => self.roots(),
        };
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for (prefix, base) in scope {
            let mut found = Vec::new();
            walk(&base, &mut found);
            for f in found {
                if files.len() >= MAX_FILES {
                    break;
                }
                let rel = f.strip_prefix(&base).unwrap_or(&f).to_string_lossy().replace('\\', "/");
                files.push((format!("{prefix}/{rel}"), f));
            }
        }
        let mut hits: Vec<(usize, String, String)> = Vec::new();
        for (display, path) in files {
            let Some(text) = read_text(&path) else { continue };
            let lower = text.to_lowercase();
            let display_lower = display.to_lowercase();
            let mut score = 0usize;
            let mut close: Vec<String> = Vec::new();
            for w in &words {
                if display_lower.contains(w.as_str()) {
                    score += 3;
                }
                score += lower.matches(w.as_str()).count().min(5);
                let near = near_words(&lower, w);
                if !near.is_empty() {
                    score += 2;
                    close.extend(near);
                }
            }
            if score == 0 {
                continue;
            }
            let find: Vec<String> = words.iter().cloned().chain(close).collect();
            let snip = snippet(&text, &lower, &find);
            hits.push((score, display, snip));
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        hits.truncate(SEARCH_HITS);
        if hits.is_empty() {
            return Ok("No notes matched.".into());
        }
        Ok(hits.iter().map(|(_, d, s)| format!("{d}\n  {s}")).collect::<Vec<_>>().join("\n"))
    }

    pub fn read_note(&self, path: &str) -> Result<String, String> {
        let (_, c) = self.resolve(path).ok_or_else(|| REFUSED.to_string())?;
        let meta = fs::metadata(&c).map_err(|_| REFUSED.to_string())?;
        if !meta.is_file() || !has_ext(&c) || meta.len() > MAX_FILE_BYTES {
            return Err(REFUSED.into());
        }
        let text = read_text(&c).ok_or_else(|| REFUSED.to_string())?;
        if text.chars().count() > READ_CHARS {
            let cut: String = text.chars().take(READ_CHARS).collect();
            return Ok(format!("{cut}\n…[cut at {READ_CHARS} chars]"));
        }
        Ok(text)
    }

    pub fn list_folder(&self, path: &str) -> Result<String, String> {
        if matches!(path, "" | "/" | ".") {
            let names: Vec<String> = self.roots().into_iter().map(|(n, _)| n).collect();
            return Ok(names.join("\n"));
        }
        let (_, dir) = self.resolve(path).ok_or_else(|| REFUSED.to_string())?;
        if !dir.is_dir() {
            return Err(REFUSED.into());
        }
        let rd = fs::read_dir(&dir).map_err(|_| REFUSED.to_string())?;
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    return None;
                }
                let meta = fs::symlink_metadata(e.path()).ok()?;
                if meta.file_type().is_symlink() {
                    None
                } else if meta.is_dir() {
                    Some(format!("{name}/"))
                } else if meta.is_file() {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        let more = names.len() > LIST_MAX;
        names.truncate(LIST_MAX);
        if more {
            names.push("…[more]".into());
        }
        Ok(names.join("\n"))
    }

    /// Dispatch one model tool call. `arguments` is the raw JSON object the model sent.
    pub fn call(&self, name: &str, arguments: &str) -> Result<String, String> {
        let arguments = if arguments.trim().is_empty() { "{}" } else { arguments };
        let args: Value = serde_json::from_str(arguments).map_err(|_| "bad arguments".to_string())?;
        if !args.is_object() {
            return Err("bad arguments".into());
        }
        let arg = |k: &str| args.get(k).and_then(Value::as_str);
        match name {
            "search_notes" => {
                let query = arg("query").ok_or_else(|| "missing query".to_string())?;
                let folder = arg("folder").filter(|f| !f.trim().is_empty());
                self.search_notes(query, folder)
            }
            "read_note" => self.read_note(arg("path").ok_or_else(|| "missing path".to_string())?),
            "list_folder" => self.list_folder(arg("path").unwrap_or("")),
            _ => Err("unknown tool".into()),
        }
    }
}

/// The OpenRouter function definitions for the three tools.
pub fn schema() -> Vec<Value> {
    vec![
        json!({"type": "function", "function": {
            "name": "search_notes",
            "description": "Search the vault notes by words in the file name or text. Returns up to 8 hits as \"<Vault>/<path>\" with a snippet each. Optional folder limits the search to one folder, e.g. \"Fidelity/Pantry\".",
            "parameters": {"type": "object", "properties": {
                "query": {"type": "string", "description": "words to look for"},
                "folder": {"type": "string", "description": "optional folder path as \"<Vault>/<rest>\""}
            }, "required": ["query"]}
        }}),
        json!({"type": "function", "function": {
            "name": "read_note",
            "description": "Read one note by its \"<Vault>/<path>\" path, exactly as search_notes prints it. Long notes are cut.",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "note path as \"<Vault>/<rest>\""}
            }, "required": ["path"]}
        }}),
        json!({"type": "function", "function": {
            "name": "list_folder",
            "description": "List one folder by its \"<Vault>/<path>\" path. An empty path lists the vault folders.",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "folder path as \"<Vault>/<rest>\", or empty"}
            }}
        }}),
    ]
}
