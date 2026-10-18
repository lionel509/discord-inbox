//! Stormy's small-task path (#84). A money text, or a DM the chat model files as `Task`, goes to one headless
//! `claude -p` run in the vault root. Claude Code settings and the OS sandbox fence it; the prompt is not the fence.
//! The bot, not the agent, commits what changed and reports the commit SHA, so every write has an undo handle.
use crate::tools::ROOTS;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Wall-clock cap on one helper run. Typing is re-sent for the whole run (#45).
pub const TIMEOUT: Duration = Duration::from_secs(180);
pub const TOOLS: &str = "Read,Edit,Write,Bash";
/// Lionel chose MiMo for this path on 2026-10-17. `DISCORD_INBOX_TASK_MODEL` switches it.
pub const MODEL: &str = "xiaomi/mimo-v2.6-flash";
/// The local claude-router, the same one interactive `claude` uses.
pub const ROUTER: &str = "http://127.0.0.1:8787";
pub const LEDGER_PY: &str = "State Street/.claude/skills/ledger/scripts/ledger.py";
pub const FINANCE: &str = "State Street";

/// Stormy's small-task path is on unless `DISCORD_INBOX_TASKS=off`.
pub fn enabled() -> bool {
    std::env::var("DISCORD_INBOX_TASKS").map_or(true, |v| v.trim() != "off")
}

pub fn model() -> String {
    env_nonempty("DISCORD_INBOX_TASK_MODEL").unwrap_or_else(|| MODEL.to_string())
}

pub fn router() -> String {
    env_nonempty("DISCORD_INBOX_ROUTER").unwrap_or_else(|| ROUTER.to_string())
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The one ledger command the helper may run. Pinned to `--vault` under the root, so a scratch root never reaches the real finance vault.
pub fn ledger_cmd(root: &Path) -> String {
    let r = root.display();
    format!("python3 \"{r}/{LEDGER_PY}\" --vault \"{r}/{FINANCE}\"")
}

/// The fixed system prompt the helper gets (`--append-system-prompt`).
pub fn helper(root: &Path) -> String {
    let cmd = ledger_cmd(root);
    let text = r#"You are Stormy's small-task helper, running headless on one Discord message from Lionel, inside his notes folder. Follow the CLAUDE.md files here. Do the task, then reply in 1 to 6 short lines naming each file you changed, or the answer, or your one question. Rules: no subagents, no routing log, no git (the bot commits and reports the SHA), one command per Bash call, never `&&`, `;` or `|`. Money goes through the ledger script and only through this exact command prefix, copied verbatim: {cmd} <subcommand> ... First run `{cmd} balance` and read the names it prints. A bank text like 'X sent you $N' (Zelle, Venmo) is money received: match X to a ledger name ignoring case, middle initials and order; if exactly one name matches, book it with `{cmd} payment --person "<ledger spelling>" --usd <N> --method <zelle|venmo> --what "<short note>"`. If no name or several match, ask one question and change nothing. Money going out ('I'm sending X back $N', 'paying X $N') is only booked when the ledger already holds the charge or split it settles (check `balance` and the entry files under State Street/Assets/Receivables/Ledger/Entries); otherwise ask one short question naming what is missing (for example whether to record the split first) and change nothing. Never invent a person, an amount or a date. Numbers in the message may be masked; never try to recover them. Never include card or account numbers. Answer in the message's language."#;
    text.replace("{cmd}", &cmd)
}

/// Whether a text DM is money the helper should handle: a bank receipt text, or a money out or split line.
pub fn wanted(text: &str) -> bool {
    let t = text.to_lowercase();
    let money_in = ["sent you $", "paid you $", "you received $"].iter().any(|k| t.contains(k))
        || (t.contains('$') && (t.contains("zelle") || t.contains("venmo")));
    let digit = t.chars().any(|c| c.is_ascii_digit());
    let money_words = [" owe", "owes", "split", "sending", "paying", "paid", "pay back", "send back", "ledger", "zelle", "venmo", "cashapp"]
        .iter()
        .any(|k| t.contains(k));
    money_in || (digit && money_words)
}

/// Every top-level folder of the root that is not an editable vault and not the inbox. Symlinked folders count.
/// The private folder is never named here: it is simply not in `ROOTS`.
pub fn deny_dirs(root: &Path, inbox: &Path) -> Vec<PathBuf> {
    let inbox_name = if inbox.parent() == Some(root) { inbox.file_name().map(|n| n.to_os_string()) } else { None };
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_s = name.to_string_lossy().to_string();
            if ROOTS.contains(&name_s.as_str()) || inbox_name.as_deref() == Some(name.as_os_str()) {
                continue;
            }
            out.push(root.join(name));
        }
    }
    out.sort();
    out
}

/// A path in permission-rule syntax: `//` plus the absolute path without its leading slash, glob characters escaped.
fn rule(p: &Path) -> String {
    let s: String = p
        .to_string_lossy()
        .trim_start_matches('/')
        .chars()
        .flat_map(|c| if "*?[]\\".contains(c) { vec!['\\', c] } else { vec![c] })
        .collect();
    format!("//{s}")
}

/// The `--settings` JSON: the editable roots and the one ledger command are allowed; every other top-level folder is
/// denied to Read and Edit, and to the OS sandbox for Bash and its children. Hooks are off; the sandbox fails closed.
pub fn settings_json(root: &Path, inbox: &Path) -> String {
    let foreign = deny_dirs(root, inbox);
    let mut allow: Vec<String> = ROOTS.iter().map(|r| format!("Edit({}/**)", rule(&root.join(r)))).collect();
    allow.push(format!("Bash({} *)", ledger_cmd(root)));
    let mut deny: Vec<String> = Vec::new();
    for d in &foreign {
        deny.push(format!("Read({}/**)", rule(d)));
    }
    for d in &foreign {
        deny.push(format!("Edit({}/**)", rule(d)));
    }
    for cmd in ["rm", "git", "curl", "wget", "pip", "pip3", "uv", "brew", "npm", "sudo", "ssh", "open"] {
        deny.push(format!("Bash({cmd} *)"));
    }
    let names: Vec<String> = foreign.iter().map(|d| d.to_string_lossy().to_string()).collect();
    serde_json::json!({
        "disableAllHooks": true,
        "permissions": { "allow": allow, "deny": deny },
        "sandbox": {
            "enabled": true,
            "autoAllowBashIfSandboxed": false,
            "allowUnsandboxedCommands": false,
            "failIfUnavailable": true,
            "filesystem": { "denyRead": names, "denyWrite": names },
            "network": { "allowedDomains": [] }
        }
    })
    .to_string()
}

/// The arguments for `/usr/bin/env` to run `claude`. The router URL goes in the environment of that one process.
pub fn argv(bin: &Path, root: &Path, inbox: &Path) -> Vec<String> {
    vec![
        format!("ANTHROPIC_BASE_URL={}", router()),
        bin.to_string_lossy().to_string(),
        "-p".into(),
        "--model".into(),
        model(),
        "--strict-mcp-config".into(),
        "--no-session-persistence".into(),
        "--setting-sources".into(),
        "project".into(),
        "--permission-mode".into(),
        "dontAsk".into(),
        "--tools".into(),
        TOOLS.into(),
        "--settings".into(),
        settings_json(root, inbox),
        "--append-system-prompt".into(),
        helper(root),
    ]
}

/// One helper run in the root, with the prompt on stdin. `None` on no binary, timeout, failure or empty output.
pub fn run(root: &Path, inbox: &Path, prompt: &str, timeout: Duration) -> Option<String> {
    let bin = crate::insight::claude_path()?;
    let args = argv(&bin, root, inbox);
    crate::ocr::run_in(Path::new("/usr/bin/env"), &args, prompt.as_bytes(), timeout, Some(root))
}

/// The changed paths of each editable vault that is a git repo, before or after the run.
pub type Snapshot = BTreeMap<String, BTreeSet<String>>;

pub fn snapshot(root: &Path) -> Snapshot {
    let mut out = Snapshot::new();
    for r in ROOTS {
        let dir = root.join(r);
        if dir.join(".git").exists() {
            out.insert(r.to_string(), changed(&dir));
        }
    }
    out
}

/// Modified, deleted and untracked (not ignored) paths, NUL-separated so nothing is quoted or trimmed.
fn changed(dir: &Path) -> BTreeSet<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["ls-files", "-z", "--modified", "--deleted", "--others", "--exclude-standard"])
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).split('\0').filter(|p| !p.is_empty()).map(String::from).collect(),
        _ => BTreeSet::new(),
    }
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git").current_dir(dir).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Commits, per vault, only the paths that became changed during the run. Returns `(vault, short sha, paths)` for each commit.
pub fn commit_changes(root: &Path, before: &Snapshot, message: &str) -> Vec<(String, String, Vec<String>)> {
    let mut done = Vec::new();
    for (vault, now) in snapshot(root) {
        let old = before.get(&vault).cloned().unwrap_or_default();
        let paths: Vec<String> = now.difference(&old).cloned().collect();
        if paths.is_empty() {
            continue;
        }
        let dir = root.join(&vault);
        let mut add: Vec<&str> = vec!["add", "--"];
        add.extend(paths.iter().map(String::as_str));
        let mut commit: Vec<&str> = vec!["commit", "-q", "-m", message, "--"];
        commit.extend(paths.iter().map(String::as_str));
        let sha = (git_ok(&dir, &add) && git_ok(&dir, &commit)).then(|| {
            let o = Command::new("git").current_dir(&dir).args(["rev-parse", "--short", "HEAD"]).stderr(Stdio::null()).output();
            o.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });
        match sha.flatten() {
            Some(sha) if !sha.is_empty() => done.push((vault, sha, paths)),
            _ => eprintln!("discord-inbox: commit in {vault} failed"),
        }
    }
    done
}
