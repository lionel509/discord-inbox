//! Vault targets, the model decision parser and note helpers (#31).
//!
//! The model only ever names a *key* from `TARGETS`. Its text never becomes
//! part of a path: `target` is an exact (case-insensitive) match against the
//! table, and `dest_dir` refuses anything that is not a real folder directly
//! under the vault root.
use std::fs;
use std::path::PathBuf;

pub struct Target {
    pub key: &'static str,
    pub folder: &'static str,
    pub hub: &'static str,
    pub tag: &'static str,
    pub hint: &'static str,
}

pub static TARGETS: [Target; 8] = [
    Target { key: "Citadel/Ideas", folder: "Citadel/Ideas", hub: "Ideas", tag: "citadel", hint: "software, electronics, robotics, 3D printing, AI tooling, a project idea" },
    Target { key: "Fidelity/Want to Make", folder: "Fidelity/Want to Make", hub: "Home", tag: "fidelity", hint: "recipes, cooking, food, something to make by hand" },
    Target { key: "BlackRock", folder: "BlackRock", hub: "Home", tag: "blackrock", hint: "school: courses, homework, exams, campus admin" },
    Target { key: "Berkshire", folder: "Berkshire", hub: "Home", tag: "berkshire", hint: "business, the store, products, vendors, channels" },
    Target { key: "Goldman", folder: "Goldman", hub: "Home", tag: "goldman", hint: "career: jobs, internships, resumes, recruiters" },
    Target { key: "State Street", folder: "State Street", hub: "Home", tag: "finance", hint: "money: purchases, receipts, accounts, debts, taxes" },
    Target { key: "Mirae", folder: "Mirae", hub: "Home", tag: "mirae", hint: "Korean or Chinese language study" },
    Target { key: "! Todo", folder: "! Todo", hub: "Home", tag: "todo", hint: "a task or reminder" },
];

pub const PROMPT_CHARS: usize = 4000;

pub struct Vaults {
    root: PathBuf,
}

pub struct Decision {
    pub target: Option<&'static Target>,
    pub title: String,
    pub tags: Vec<String>,
    pub reply: String,
}

impl Vaults {
    pub fn new(root: PathBuf) -> Vaults {
        Vaults { root }
    }

    pub fn default_root() -> PathBuf {
        if let Some(p) = std::env::var_os("DISCORD_INBOX_VAULTS_DIR") {
            return PathBuf::from(p);
        }
        let home = std::env::var_os("HOME").unwrap_or_default();
        PathBuf::from(home).join("Documents")
    }

    /// Exact, case-insensitive match against `TARGETS`. `Inbox` and anything
    /// else not in the table give `None`.
    pub fn target(&self, key: &str) -> Option<&'static Target> {
        let s = key.trim();
        let s = s.trim_matches(|c| c == '`' || c == '"' || c == '\'').trim();
        if s.is_empty() {
            return None;
        }
        TARGETS.iter().find(|t| t.key.eq_ignore_ascii_case(s))
    }

    /// The real folder `root/<folder>`, or `None`. Never creates anything; a
    /// symlink that resolves anywhere other than `root/<folder>` is refused.
    pub fn dest_dir(&self, t: &Target) -> Option<PathBuf> {
        let d = self.root.join(t.folder);
        let meta = fs::symlink_metadata(&d).ok()?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return None;
        }
        let real = d.canonicalize().ok()?;
        let want = self.root.canonicalize().ok()?.join(t.folder);
        if real != want {
            return None;
        }
        Some(d)
    }
}

pub fn system_prompt() -> String {
    let mut s = String::from(
        "You file one message from the owner into his vault and reply to it.\n\
         Pick the folder whose key fits best. Keys:\n",
    );
    for t in TARGETS.iter() {
        s.push_str(&format!("{} — {}\n", t.key, t.hint));
    }
    s.push_str(
        "Inbox — personal life: health, people, relationships, feelings, schedules, or when unsure\n\n\
         Answer in exactly this format:\n\
         FOLDER: <one key from the list above, or Inbox>\n\
         TITLE: <3 to 8 words>\n\
         TAGS: <up to 3 comma-separated lowercase words>\n\
         REPLY:\n\
         <1 to 3 sentences: answer the message if it asks something, else say what it is>\n\n\
         Reply in the user's language. Never include card numbers.",
    );
    s
}

pub fn user_prompt(content: &str, ocr: &[String]) -> String {
    let mut s = String::from(content);
    for block in ocr {
        s.push_str("\n\nAttached text:\n");
        s.push_str(block);
    }
    s.chars().take(PROMPT_CHARS).collect()
}

const LABELS: [&str; 3] = ["folder:", "title:", "tags:"];

pub fn parse_decision(vaults: &Vaults, text: &str) -> Decision {
    let mut target = None;
    let mut title = String::new();
    let mut tags = Vec::new();
    let mut loose: Vec<&str> = Vec::new();
    let mut reply_lines: Option<Vec<&str>> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if let Some(lines) = reply_lines.as_mut() {
            lines.push(line);
            continue;
        }
        if lower.starts_with("reply:") {
            let rest = trimmed["reply:".len()..].trim();
            reply_lines = Some(if rest.is_empty() { Vec::new() } else { vec![rest] });
            continue;
        }
        if let Some(label) = LABELS.iter().find(|l| lower.starts_with(*l)) {
            let rest = &trimmed[label.len()..];
            match *label {
                "folder:" => {
                    if target.is_none() {
                        target = vaults.target(rest);
                    }
                }
                "title:" => {
                    if title.is_empty() {
                        title = clean_title(rest);
                    }
                }
                _ => {
                    if tags.is_empty() {
                        tags = clean_tags(rest);
                    }
                }
            }
            continue;
        }
        loose.push(line);
    }

    let reply = match reply_lines {
        Some(lines) => lines.join("\n"),
        None => loose.join("\n"),
    };
    let reply = reply.trim().to_string();
    let reply = if reply.is_empty() { "Noted.".to_string() } else { reply };
    Decision { target, title, tags, reply }
}

/// Letters, digits, space, `-` and `_` only; runs of spaces collapsed; at most
/// 60 chars; no leading dots.
fn clean_title(s: &str) -> String {
    let kept: String = s
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect();
    let joined = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(60).collect();
    cut.trim().trim_start_matches('.').trim().to_string()
}

/// Lowercase, `[a-z0-9-]` only, no empties or repeats, at most 3.
fn clean_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in s.split(',') {
        let tag: String = raw
            .trim()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect();
        if tag.is_empty() || out.contains(&tag) {
            continue;
        }
        out.push(tag);
        if out.len() == 3 {
            break;
        }
    }
    out
}

/// `2026-10-13 Solar fan.md`, or `2026-10-13 Note 140322.md` when the title is empty.
pub fn note_name(received: &str, title: &str) -> String {
    let date: String = received.chars().take(10).collect();
    let clean = clean_title(title);
    if !clean.is_empty() {
        return format!("{date} {clean}.md");
    }
    let hms: String = received
        .chars()
        .skip(11)
        .take(8)
        .filter(|c| c.is_ascii_digit())
        .collect();
    format!("{date} Note {hms}.md")
}

/// Bare YAML list: the domain tag, `discord-inbox`, then the topic tags.
pub fn front_extra(t: &Target, tags: &[String]) -> String {
    let mut s = format!("tags:\n  - {}\n  - discord-inbox\n", t.tag);
    for tag in tags {
        s.push_str(&format!("  - {tag}\n"));
    }
    s
}

pub fn hub_line(t: &Target) -> String {
    format!("Hub: [[{}]]\n\n", t.hub)
}
