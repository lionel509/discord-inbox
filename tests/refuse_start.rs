use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("di-refuse-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_with_allowlist(allow: &PathBuf, inbox: &PathBuf) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_discord-inbox"))
        .env("DISCORD_INBOX_ALLOWED_IDS_FILE", allow)
        .env("DISCORD_INBOX_DIR", inbox)
        .output()
        .unwrap()
}

#[test]
fn missing_allowlist_refuses_start() {
    let dir = scratch("missing");
    let allow = dir.join("nope");
    let inbox = dir.join("inbox");
    let out = run_with_allowlist(&allow, &inbox);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("allowed_ids"));
    assert!(!inbox.exists());
}

#[test]
fn empty_allowlist_refuses_start() {
    let dir = scratch("empty");
    let allow = dir.join("allowed_ids");
    fs::write(&allow, "# no ids\n").unwrap();
    let inbox = dir.join("inbox");
    let out = run_with_allowlist(&allow, &inbox);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("allowed_ids"));
    assert!(!inbox.exists());
}

const ALLOWED: &str = "100000000000000001\n";

/// Runs the binary with a fake HOME, so the login Keychain item is never found and a start
/// that gets past the guards stops at the token step. It never reaches the gateway.
fn run_guarded(
    cwd: &PathBuf,
    home: Option<&PathBuf>,
    inbox: Option<&str>,
    allow: &PathBuf,
) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_discord-inbox"));
    cmd.current_dir(cwd)
        .env("DISCORD_INBOX_ALLOWED_IDS_FILE", allow);
    match inbox {
        Some(v) => cmd.env("DISCORD_INBOX_DIR", v),
        None => cmd.env_remove("DISCORD_INBOX_DIR"),
    };
    match home {
        Some(h) => cmd.env("HOME", h),
        None => cmd.env_remove("HOME"),
    };
    cmd.output().unwrap()
}

#[test]
fn relative_or_empty_inbox_refuses_start() {
    let dir = scratch("relative");
    let allow = dir.join("allowed_ids");
    fs::write(&allow, ALLOWED).unwrap();
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let cwd = dir.join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    let cases: [(&str, Option<&PathBuf>, Option<&str>); 3] = [
        ("empty DISCORD_INBOX_DIR", Some(&home), Some("")),
        ("relative DISCORD_INBOX_DIR", Some(&home), Some("rel/Inbox")),
        ("HOME unset, default path", None, None),
    ];
    for (case, home, inbox) in cases {
        let out = run_guarded(&cwd, home, inbox, &allow);
        assert!(!out.status.success(), "{case}: started");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("absolute"),
            "{case}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let written: Vec<_> = fs::read_dir(&cwd).unwrap().collect();
        assert!(written.is_empty(), "{case}: wrote into cwd: {written:?}");
    }
}

#[test]
fn second_instance_refuses_start_while_lock_is_held() {
    let dir = scratch("locked");
    let allow = dir.join("allowed_ids");
    fs::write(&allow, ALLOWED).unwrap();
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let inbox = dir.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    // The test plays the first instance: it holds the exclusive lock for the whole run.
    let holder = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(inbox.join(".lock"))
        .unwrap();
    holder.try_lock().unwrap();
    let out = run_guarded(&dir, Some(&home), inbox.to_str(), &allow);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("already running"), "{err}");
    assert!(!err.contains("keychain"), "went past the lock: {err}");
    let names: Vec<String> = fs::read_dir(&inbox)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![".lock".to_string()],
        "second instance wrote files"
    );
    drop(holder);
}

#[test]
fn start_with_lock_released_passes_the_guard() {
    let dir = scratch("released");
    let allow = dir.join("allowed_ids");
    fs::write(&allow, ALLOWED).unwrap();
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let inbox = dir.join("Inbox");
    let out = run_guarded(&dir, Some(&home), inbox.to_str(), &allow);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!err.contains("already running"), "{err}");
    // Past the lock, the next step is the Keychain read, which fails under the fake HOME.
    assert!(err.contains("keychain"), "{err}");
}
