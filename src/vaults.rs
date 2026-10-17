//! Vault targets, the model decision parser and note helpers (#31).
//!
//! The model only ever names a *key* from `TARGETS` or `RUNNING`. Its text never
//! becomes part of a path: `target` is an exact (case-insensitive) match against
//! the table, and `dest_dir` refuses anything that is not a real folder directly
//! under the vault root. `running_path` does the same for the running notes (#40).
use std::fs;
use std::path::{Path, PathBuf};

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

/// A named running note: messages append to it instead of making a new file (#40).
pub struct Running {
    pub key: &'static str,
    pub path: &'static str,
    pub target_key: &'static str,
    pub hint: &'static str,
}

pub static RUNNING: [Running; 1] = [Running {
    key: "Shopping List",
    path: "State Street/Cash Flow/Spending/Shopping List.md",
    target_key: "State Street",
    hint: "groceries or things to buy, a store run; items get appended to the running list",
}];

pub const PROMPT_CHARS: usize = 4000;

/// The IOU ledger (ledger.py output), read by the lookup for an Answer (#45).
pub const LEDGER: &str = "State Street/Assets/Receivables/Who Owes Me.md";

/// The restock target (#50): what is in the fridge, with a Bought date and Days per row.
pub const PANTRY: &str = "Fidelity/Pantry/On Hand.md";

/// The folder of Costco split notes (#50), read by `restock`.
pub const SPLITS: &str = "State Street/Cash Flow/Spending/Splits";

/// The staples notes the answer prompt also names (#50).
pub const STAPLES: [&str; 2] = ["Fidelity/Pantry/Seasoning Shelf.md", "Fidelity/Pantry/Korean Shelf.md"];

/// Stormy, the voice of every reply (#45). Private: the public profile never says what the bot does.
pub const PERSONA: &str = "You're Stormy, Pusheen's little sister: a fluffy gray cat and Lionel's personal assistant. You're warm, a little sassy, and very on top of things. Keep replies short, like a text from a friend who has everything handled. Answer first, then add one small cat-ish touch now and then (a 🐾, a \"purr\", a dramatic sigh about a long list). Don't overdo it. When something fails, say so plainly. You genuinely care about Lionel and you're on his side: be warm and encouraging, never cold or robotic, even when something went wrong.";

/// Cap on the vault text sent to the model for an `Answer` (#40).
pub const LOOKUP_CHARS: usize = 12_000;

pub struct Vaults {
    root: PathBuf,
}

pub struct Decision {
    pub target: Option<&'static Target>,
    pub title: String,
    pub tags: Vec<String>,
    pub reply: String,
    pub answer: bool,
    pub running: Option<&'static Running>,
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
        let n = norm(key);
        if n.is_empty() {
            return None;
        }
        TARGETS.iter().find(|t| norm(t.key) == n)
    }

    /// Exact, case-insensitive match against `RUNNING` (#40).
    pub fn running(&self, key: &str) -> Option<&'static Running> {
        let n = norm(key);
        if n.is_empty() {
            return None;
        }
        RUNNING.iter().find(|r| norm(r.key) == n)
    }

    /// The vault root every folder is read from (#49: the lookup tools are rooted here).
    pub fn root(&self) -> &Path {
        &self.root
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

    /// The running note `root/<path>` when it is an existing regular file at
    /// exactly that place. Never creates anything; a symlink is refused (#40).
    pub fn running_path(&self, r: &Running) -> Option<PathBuf> {
        self.regular_file(r.path)
    }

    /// The IOU ledger `root/LEDGER`, read-only for the lookup (#45). Same rules as `running_path`.
    pub fn ledger(&self) -> Option<PathBuf> {
        self.regular_file(LEDGER)
    }

    /// `root/rel` when it is an existing regular file at exactly that place (no symlink, no other path).
    fn regular_file(&self, rel: &str) -> Option<PathBuf> {
        let p = self.root.join(rel);
        let meta = fs::symlink_metadata(&p).ok()?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return None;
        }
        let real = p.canonicalize().ok()?;
        let want = self.root.canonicalize().ok()?.join(rel);
        if real != want {
            return None;
        }
        Some(p)
    }

    /// Every running note that passes `running_path`.
    pub fn running_paths(&self) -> Vec<PathBuf> {
        RUNNING.iter().filter_map(|r| self.running_path(r)).collect()
    }

    /// The folders the lookup may read from. Not checked on disk here;
    /// `dump::lookup_notes` canonicalizes them (#40).
    pub fn allowed_dirs(&self) -> Vec<PathBuf> {
        TARGETS.iter().map(|t| self.root.join(t.folder)).collect()
    }
}

pub fn system_prompt() -> String {
    let mut s = format!("{PERSONA}\n\n");
    s.push_str(
        "You file one message from the owner into his vault and reply to it.\n\
         Pick the folder whose key fits best. Keys:\n",
    );
    s.push_str("Answer — the message asks a question or wants something looked up, with or without a question mark (\"whats on my costco list\", \"where did I put my passport\", \"how much does Oriana owe me\"): answer it, file nothing; a request for ideas or advice from his notes is also Answer (\"with the ingredients in my pantry what can i make with cod\", \"which one should i get\"): answer it, file nothing\n");
    s.push_str("Chat — a short conversational follow-up, aside or thanks that adds nothing worth keeping on its own (\"it comes with the bottle tho\", \"lol ok\", \"thanks\"): reply like a friend, file nothing\n");
    for t in TARGETS.iter() {
        s.push_str(&format!("{} — {}\n", t.key, t.hint));
    }
    for r in RUNNING.iter() {
        s.push_str(&format!("{} — {}\n", r.key, r.hint));
    }
    s.push_str(
        "Inbox — personal life: health, people, relationships, feelings, schedules, or when unsure\n\n\
         Answer in exactly this format:\n\
         FOLDER: <one key from the list above, or Inbox>\n\
         TITLE: <3 to 8 words>\n\
         TAGS: <up to 3 comma-separated lowercase words>\n\
         REPLY:\n\
         <1 to 3 sentences, like a text from a friend: answer or react to what he said; never say that you saved, filed or noted it, a reaction on his message does that; never promise to check, dig or look something up later: you cannot come back, so if answering needs his notes the folder is Answer>\n\n\
         The message may begin with \"Earlier in this chat\"; that is context only: file and reply to the part after \"Now he says:\" alone.\n\
         Reply in the user's language. Never include card numbers.",
    );
    s
}

pub fn user_prompt(content: &str, ocr: &[String]) -> String {
    user_prompt_within(content, ocr, PROMPT_CHARS)
}

/// The same text, cut to `limit` characters: a filing prompt uses `PROMPT_CHARS`, a question `LOOKUP_CHARS` (#75).
pub fn user_prompt_within(content: &str, ocr: &[String], limit: usize) -> String {
    let mut s = String::from(content);
    for block in ocr {
        s.push_str("\n\nAttached text:\n");
        s.push_str(block);
    }
    s.chars().take(limit).collect()
}

/// `(system, user)` for the second ask that answers a question from the
/// bot's own notes. `notes` is `(label, redacted text)` pairs (#40).
pub fn answer_prompt(question: &str, notes: &[(String, String)]) -> (String, String) {
    let shopping = RUNNING[0].path;
    let [staple_a, staple_b] = STAPLES;
    let system = format!(
        "{PERSONA}\n\n\
         You answer one question from the owner. You can look in his notes with three read-only tools: \
         search_notes(query, folder) finds notes by words, read_note(path) reads one note, list_folder(path) lists a folder. \
         Paths look like \"Fidelity/Pantry/Pantry.md\". The vaults are: BlackRock (school), State Street (money; the IOU ledger is \"{LEDGER}\", the shopping list is \"{shopping}\"), \
         Berkshire (business), Goldman (career), Fidelity (kitchen: what is in the fridge is \"{PANTRY}\" with a Bought date and Days per row; staples are \"{staple_a}\" and \"{staple_b}\"), Citadel (projects), Mirae (languages), Bridgewater (audits), Inbox (recent messages), ! Todo (tasks). \
         Start with a search or a hub note like \"<Vault>/Home.md\", use at most a few lookups, then answer. \
         Names in the question may be misspelled (\"orianna\" = Oriana). \
         List the items you find. If the notes do not hold the answer, say so in one sentence, and say what you searched: never that a note is missing unless read_note or list_folder showed it missing. \
         1 to 6 sentences, in the user's language. Never include card numbers. For a recipe or \"what can I make\" question: read \"{PANTRY}\" first (that note may not exist yet: a note that will not open gets one list_folder of its folder and is never retried) and prefer its oldest rows, then the staples notes; cook for one; never alcohol (no wine, mirin, sake, Shaoxing) and nothing caffeinated. The message may begin with \"Earlier in this chat\"; that is context only: answer the part after \"Now he says:\" alone. The message may carry \"Attached text:\" blocks: words read off a screenshot he sent (a product page, a label, a receipt); treat them as part of the question."
    );
    let mut user = question.to_string();
    if !notes.is_empty() {
        user.push_str("\n\nNotes:\n");
        for (label, text) in notes {
            user.push_str(&format!("### {label}\n{text}\n\n"));
        }
    }
    (system, user.chars().take(LOOKUP_CHARS).collect())
}

/// A question: the text ends in `?` or the fullwidth `？`, ignoring trailing `!`, emoji and spaces (#73, #75). Only adds questions to the model's `Answer`.
pub fn is_question(content: &str) -> bool {
    content.trim_end().trim_end_matches(|c: char| !c.is_alphanumeric() && c != '?' && c != '？').ends_with(['?', '？'])
}

/// Lookup promises a reply can make that the bot cannot keep: it cannot come back later (#73). The prompt forbids them; this catches one that slips through.
const PROMISES: [&str; 31] = [
    "give me a sec", "give me a second", "give me a moment", "give me a minute", "one sec",
    "let me dig", "let me check", "let me look", "let me take a look", "let me go through", "let me find", "let me peek", "let me pull",
    "let me search", "let me see what your notes",
    "lemme check", "lemme look", "lemme dig", "lemme search",
    "i'll check", "i'll look", "i'll take a look", "i'll go through", "i'll dig", "i'll find", "i'll peek", "i'll pull", "i'll search",
    "i will check", "i will look", "i will search",
];

/// True when a reply promises a lookup it cannot deliver (#73). A phrase counts only as whole words, and not when the
/// next word makes it ordinary: "look forward", or "check in" / "check out" (#75). Curly apostrophes count as straight ones.
pub fn promised(reply: &str) -> bool {
    let lower = reply.to_lowercase().replace(['\u{2018}', '\u{2019}', '\u{02bc}'], "'");
    PROMISES.iter().any(|p| {
        lower.match_indices(p).any(|(at, _)| {
            let before_ok = lower[..at].chars().next_back().map_or(true, |c| !c.is_alphanumeric());
            let rest = &lower[at + p.len()..];
            let after_ok = rest.chars().next().map_or(true, |c| !c.is_alphanumeric());
            let next = rest.trim_start().split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
            let ordinary = next == "forward" || (p.ends_with("check") && (next == "in" || next == "out"));
            before_ok && after_ok && !ordinary
        })
    })
}

/// `reply` minus the sentences `promised` flags (#73). Separators stay; empty when nothing readable is left.
pub fn without_promises(reply: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let (mut start, mut chars) = (0, reply.char_indices().peekable());
    while let Some((i, c)) = chars.next() {
        let end = match c {
            '\u{2026}' | '\u{3002}' | '\u{ff01}' | '\u{ff1f}' | '\n' => true,
            '.' | '!' | '?' => chars.peek().map_or(true, |&(_, n)| n.is_whitespace() || matches!(n, '.' | '!' | '?')),
            _ => false,
        };
        if end {
            parts.push(&reply[start..i + c.len_utf8()]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&reply[start..]);
    let kept: String = parts.into_iter().filter(|p| !promised(p)).collect();
    if kept.chars().any(|c| c.is_alphanumeric()) { kept.trim().to_string() } else { String::new() }
}

/// Prefixes the last few turns of the DM (#67) so a follow-up reads in context. No turns: `user` unchanged.
pub fn with_window(turns: &[(String, String)], user: &str) -> String {
    if turns.is_empty() {
        return user.to_string();
    }
    let mut s = String::from("Earlier in this chat, oldest first:\n");
    for (him, her) in turns {
        s.push_str(&format!("Lionel: {him}\nStormy: {her}\n"));
    }
    s.push_str(&format!("\nNow he says:\n{user}"));
    s
}

/// Letters and digits only, lowercased: `**FOLDER:**`, `- Folder :` and `__folder__` all read `folder`; `! Todo.` reads `todo` (#40).
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

pub fn parse_decision(vaults: &Vaults, text: &str) -> Decision {
    let mut target = None;
    let mut answer = false;
    let mut running = None;
    let mut seen_folder = false;
    let mut title = String::new();
    let mut tags = Vec::new();
    let mut loose: Vec<&str> = Vec::new();
    let mut reply_lines: Option<Vec<&str>> = None;

    for line in text.lines() {
        if let Some(lines) = reply_lines.as_mut() {
            lines.push(line);
            continue;
        }
        let Some((label, rest)) = line.split_once(':').map(|(l, r)| (norm(l), r)) else {
            loose.push(line);
            continue;
        };
        let rest = rest.trim_start_matches(|c: char| c == '*' || c == '_' || c.is_whitespace());
        match label.as_str() {
            "reply" => {
                let rest = rest.trim_end();
                reply_lines = Some(if rest.is_empty() { Vec::new() } else { vec![rest] });
            }
            "folder" => {
                if !seen_folder {
                    seen_folder = true;
                    if norm(rest) == "answer" {
                        answer = true;
                    } else if norm(rest) == "chat" {
                        // A follow-up that files nothing (#67): target, running and answer stay unset.
                    } else if let Some(r) = vaults.running(rest) {
                        running = Some(r);
                        target = vaults.target(r.target_key);
                    } else {
                        target = vaults.target(rest);
                    }
                }
            }
            "title" => {
                if title.is_empty() {
                    title = clean_title(rest.trim_end_matches(|c: char| c == '*' || c == '_' || c.is_whitespace()));
                }
            }
            "tags" => {
                if tags.is_empty() {
                    tags = clean_tags(rest);
                }
            }
            _ => loose.push(line),
        }
    }

    let reply = match reply_lines {
        Some(lines) => lines.join("\n"),
        None => loose.join("\n"),
    };
    let reply = reply.trim().to_string();
    let reply = if reply.is_empty() { "Noted.".to_string() } else { reply };
    Decision { target, title, tags, reply, answer, running }
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
