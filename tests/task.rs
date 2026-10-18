// Stormy's small-task path (#84): the sandbox settings, the pinned ledger rule, the argv, the headless run,
// the timeout, the commit of only the new changes, the money trigger and the Task folder.
use discord_inbox::task::{self};
use discord_inbox::tools::ROOTS;
use discord_inbox::vaults::{parse_decision, system_prompt, Vaults};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static ENV: Mutex<()> = Mutex::new(());

const INBOX: &str = "Discord Inbox";
// Made-up folder names; the real private folder is never named here.
const FOREIGN: [&str; 12] = [
    "Private", ".obsidian", "Old Stuff 2024", "We*ird", "q?", "[brackets]", "日本語", "Link", "Scratch Pad", "Archive 2019", "Tax Docs", "Misc",
];

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(|p| p.into_inner())
}

fn root(name: &str) -> PathBuf {
    let r = std::env::temp_dir().join(format!("di-task-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&r);
    fs::create_dir_all(&r).unwrap();
    for d in ROOTS.iter().copied().chain([INBOX]) {
        fs::create_dir_all(r.join(d)).unwrap();
    }
    for d in FOREIGN.iter().filter(|d| **d != "Link") {
        fs::create_dir_all(r.join(d)).unwrap();
    }
    std::os::unix::fs::symlink(r.join("Private"), r.join("Link")).unwrap();
    fs::write(r.join("Home.md"), "x").unwrap();
    r
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("di-task-out-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A fake `claude` that runs `body` after a shebang. Written, closed, then made executable.
fn fake(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("claude-fake");
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Escapes the glob characters the way the rule syntax needs (`\` before `*`, `?`, `[`, `]`, `\`).
fn esc(s: &str) -> String {
    s.chars().flat_map(|c| if "*?[]\\".contains(c) { vec!['\\', c] } else { vec![c] }).collect()
}

fn rule_path(p: &Path) -> String {
    esc(p.to_string_lossy().trim_start_matches('/'))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn settings(r: &Path) -> Value {
    serde_json::from_str(&task::settings_json(r, &r.join(INBOX))).unwrap()
}

#[test]
fn settings_denies_every_foreign_dir() {
    let _g = lock();
    let r = root("deny");
    let s = settings(&r);
    let deny = strings(&s["permissions"]["deny"]);
    let allow = strings(&s["permissions"]["allow"]);
    let read_list = strings(&s["sandbox"]["filesystem"]["denyRead"]);
    let write_list = strings(&s["sandbox"]["filesystem"]["denyWrite"]);

    for name in FOREIGN {
        let p = r.join(name);
        let ps = p.to_string_lossy().to_string();
        assert!(deny.contains(&format!("Read(//{}/**)", rule_path(&p))), "{name}: Read rule");
        assert!(deny.contains(&format!("Edit(//{}/**)", rule_path(&p))), "{name}: Edit rule");
        assert!(read_list.contains(&ps), "{name}: sandbox denyRead");
        assert!(write_list.contains(&ps), "{name}: sandbox denyWrite");
    }
    assert_eq!(read_list.len(), FOREIGN.len(), "sandbox read list holds exactly the foreign dirs");
    assert_eq!(write_list.len(), FOREIGN.len(), "sandbox write list holds exactly the foreign dirs");
    assert_eq!(deny.iter().filter(|d| d.starts_with("Read(")).count(), FOREIGN.len());

    for name in ROOTS.iter().copied().chain([INBOX]) {
        let p = r.join(name);
        let ps = p.to_string_lossy().to_string();
        assert!(!read_list.contains(&ps) && !write_list.contains(&ps), "{name} must stay open");
        assert!(!deny.iter().any(|d| d.contains(&format!("//{}/", rule_path(&p)))), "{name} must not be in a deny rule");
    }
    assert!(!deny.iter().chain(&read_list).chain(&write_list).any(|d| d.contains("Home.md")), "a plain file is not a folder fence");

    // Allow: an Edit rule per editable root, and the one ledger Bash rule.
    assert_eq!(allow.len(), ROOTS.len() + 1, "{allow:?}");
    for name in ROOTS {
        assert!(allow.contains(&format!("Edit(//{}/**)", rule_path(&r.join(name)))), "{name}: Edit allowed");
    }
    assert_eq!(allow.iter().filter(|a| a.starts_with("Bash(")).count(), 1);
    assert!(deny.contains(&"Bash(git *)".to_string()));
    assert!(deny.contains(&"Bash(rm *)".to_string()));

    assert_eq!(s["disableAllHooks"], Value::Bool(true));
    assert_eq!(s["sandbox"]["enabled"], Value::Bool(true));
    assert_eq!(s["sandbox"]["autoAllowBashIfSandboxed"], Value::Bool(false));
    assert_eq!(s["sandbox"]["allowUnsandboxedCommands"], Value::Bool(false));
    assert_eq!(s["sandbox"]["failIfUnavailable"], Value::Bool(true));
    assert_eq!(s["sandbox"]["network"]["allowedDomains"], serde_json::json!([]));
}

#[test]
fn ledger_is_pinned_to_root() {
    let _g = lock();
    let r = root("ledger");
    let cmd = task::ledger_cmd(&r);
    let pre = format!("python3 \"{0}/State Street/.claude/skills/ledger/scripts/ledger.py\" --vault \"{0}/State Street\"", r.display());
    assert!(cmd.starts_with(&pre), "{cmd}");

    let allow = strings(&settings(&r)["permissions"]["allow"]);
    let bash: Vec<&String> = allow.iter().filter(|a| a.starts_with("Bash(")).collect();
    assert_eq!(bash, vec![&format!("Bash({cmd} *)")]);
    assert!(!allow.iter().any(|a| a.contains("git")), "{allow:?}");

    let h = task::helper(&r);
    assert!(h.contains(&cmd));
    assert_eq!(h.matches("ledger.py").count(), h.matches(&cmd).count(), "no other ledger command in the helper prompt");
}

#[test]
fn argv_is_routed_and_isolated() {
    let _g = lock();
    std::env::remove_var("DISCORD_INBOX_TASK_MODEL");
    std::env::remove_var("DISCORD_INBOX_ROUTER");
    let r = root("argv");
    let a = task::argv(Path::new("/opt/claude"), &r, &r.join(INBOX));

    assert_eq!(a[0], "ANTHROPIC_BASE_URL=http://127.0.0.1:8787");
    assert_eq!(a[1], "/opt/claude");
    let head: Vec<&str> = a[2..14].iter().map(String::as_str).collect();
    assert_eq!(
        head,
        vec![
            "-p",
            "--model",
            "xiaomi/mimo-v2.6-flash",
            "--strict-mcp-config",
            "--no-session-persistence",
            "--setting-sources",
            "project",
            "--permission-mode",
            "dontAsk",
            "--tools",
            "Read,Edit,Write,Bash",
            "--settings",
        ]
    );
    assert!(serde_json::from_str::<Value>(&a[14]).is_ok());
    assert_eq!(a[15], "--append-system-prompt");
    assert_eq!(a[16], task::helper(&r));
    assert_eq!(a.len(), 17, "{a:?}");
    for bad in ["-u", "--bare", "--dangerously-skip-permissions", "--allowedTools", "", "--tools-off"] {
        assert!(!a.iter().any(|x| x == bad), "{bad} must not be passed");
    }

    std::env::set_var("DISCORD_INBOX_TASK_MODEL", "kimi/k3-256k");
    let a = task::argv(Path::new("/opt/claude"), &r, &r.join(INBOX));
    std::env::remove_var("DISCORD_INBOX_TASK_MODEL");
    assert_eq!(a[4], "kimi/k3-256k");
}

#[test]
fn run_uses_fake_claude_in_root_cwd() {
    let _g = lock();
    let r = root("run");
    let out = scratch("run");
    let bin = fake(
        &out,
        &format!(
            "pwd -P > '{o}/cwd'\nprintf '%s\\n' \"$@\" > '{o}/argv'\nprintf '%s' \"$ANTHROPIC_BASE_URL\" > '{o}/base'\ncat > '{o}/stdin'\nprintf 'Booked it 🐾\\n'",
            o = out.display()
        ),
    );
    std::env::set_var("DISCORD_INBOX_CLAUDE", &bin);
    let got = task::run(&r, &r.join(INBOX), "PROMPT Paco", Duration::from_secs(10));
    std::env::remove_var("DISCORD_INBOX_CLAUDE");

    assert_eq!(got.as_deref(), Some("Booked it 🐾"));
    assert_eq!(fs::read_to_string(out.join("cwd")).unwrap().trim(), r.canonicalize().unwrap().to_string_lossy());
    assert_eq!(fs::read_to_string(out.join("stdin")).unwrap(), "PROMPT Paco");
    assert_eq!(fs::read_to_string(out.join("base")).unwrap(), "http://127.0.0.1:8787");
    let argv: Vec<String> = fs::read_to_string(out.join("argv")).unwrap().lines().map(String::from).collect();
    assert!(argv.windows(2).any(|w| w[0] == "--setting-sources" && w[1] == "project"), "{argv:?}");
}

#[test]
fn timeout_kills_grandchild() {
    let _g = lock();
    let r = root("timeout");
    let out = scratch("timeout");
    let bin = fake(&out, &format!("sleep 300 &\necho $! > '{o}/pid'\nwait", o = out.display()));
    std::env::set_var("DISCORD_INBOX_CLAUDE", &bin);
    let t = Instant::now();
    let got = task::run(&r, &r.join(INBOX), "x", Duration::from_secs(1));
    std::env::remove_var("DISCORD_INBOX_CLAUDE");

    assert!(got.is_none());
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    let pid = fs::read_to_string(out.join("pid")).expect("the fake wrote its grandchild pid").trim().to_string();
    let start = Instant::now();
    loop {
        let alive = Command::new("kill").args(["-0", &pid]).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success();
        if !alive {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2), "grandchild {pid} survived the timeout");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).to_string()
}

#[test]
fn commit_only_new_changes() {
    let r = root("commit");
    let ss = r.join("State Street");
    git(&ss, &["init", "-q"]);
    git(&ss, &["config", "user.name", "Test"]);
    git(&ss, &["config", "user.email", "test@example.invalid"]);
    git(&ss, &["config", "commit.gpgsign", "false"]);
    fs::write(ss.join("Notes.md"), "a").unwrap();
    fs::write(ss.join("Other.md"), "b").unwrap();
    git(&ss, &["add", "-A"]);
    git(&ss, &["commit", "-q", "-m", "init"]);

    // Dirty before the task: must never be swept into the task's commit.
    fs::write(ss.join("Notes.md"), "a2").unwrap();
    fs::create_dir_all(r.join("! Todo")).unwrap();
    let before = task::snapshot(&r);

    let entry = "Assets/Receivables/Ledger/Entries/2026-10-18 payment — Paco Nhan $25.40.md";
    fs::create_dir_all(ss.join("Assets/Receivables/Ledger/Entries")).unwrap();
    fs::write(ss.join(entry), "p").unwrap();
    fs::write(ss.join("Other.md"), "b2").unwrap();
    fs::write(r.join("! Todo/loose.md"), "no repo here").unwrap();

    let done = task::commit_changes(&r, &before, "stormy: test");
    assert_eq!(done.len(), 1, "{done:?}");
    let (vault, sha, paths) = &done[0];
    assert_eq!(vault, "State Street");
    assert!(sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit()), "{sha}");
    let mut paths = paths.clone();
    paths.sort();
    assert_eq!(paths, vec![entry.to_string(), "Other.md".to_string()]);

    let status: Vec<String> = git(&ss, &["status", "--porcelain"]).lines().map(String::from).collect();
    assert_eq!(status, vec![" M Notes.md".to_string()]);
    assert_eq!(git(&ss, &["log", "-1", "--format=%s"]).trim(), "stormy: test");
}

#[test]
fn wanted_matches_money_texts() {
    for t in [
        "Chase | Zelle(R): PACO M NHAN sent you $25.40 & it's ready now.",
        "Kevin paid you $12 on Venmo",
        "im sending paco back 50 cus he split the 3d printer fialment with me and he used none of it so far",
        "does kevin owe me 20",
    ] {
        assert!(task::wanted(t), "{t}");
    }
    for t in ["how much does willam owe me", "whats on my costco list", "lol ok", "$5 for the thing", "im back in 5", "send me 3 ideas"] {
        assert!(!task::wanted(t), "{t}");
    }
}

#[test]
fn task_folder_is_a_task_not_an_answer() {
    let r = root("decision");
    let v = Vaults::new(r.clone());
    let d = parse_decision(&v, "FOLDER: Task\nTITLE: x\nTAGS: a\nREPLY:\nok");
    assert!(d.task);
    assert!(d.target.is_none());
    assert!(!d.answer);
    let a = parse_decision(&v, "FOLDER: Answer\nREPLY:\nok");
    assert!(a.answer && !a.task);
    assert!(system_prompt().contains("Task — "));
}
