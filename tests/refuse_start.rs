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
