//! U4: pip-bot refuses to start on bad configuration, before it touches the keychain (pattern of tests/refuse_start.rs).
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pip-refuse-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn valid_allowlist(root: &PathBuf) -> PathBuf {
    let p = root.join("allowed_ids");
    fs::write(&p, "100000000000000001\n").unwrap();
    p
}

#[test]
fn unset_nutrition_dir_refuses_start_and_names_the_variable() {
    let root = scratch("unset");
    let out = Command::new(env!("CARGO_BIN_EXE_pip-bot"))
        .env("DISCORD_INBOX_ALLOWED_IDS_FILE", valid_allowlist(&root))
        .env_remove("PIP_NUTRITION_DIR")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("PIP_NUTRITION_DIR"));
}

#[test]
fn relative_nutrition_dir_refuses_start() {
    let root = scratch("relative");
    let out = Command::new(env!("CARGO_BIN_EXE_pip-bot"))
        .env("DISCORD_INBOX_ALLOWED_IDS_FILE", valid_allowlist(&root))
        .env("PIP_NUTRITION_DIR", "relative/Nutrition")
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn second_instance_refuses_start_while_the_lock_is_held() {
    let root = scratch("locked");
    let dir = root.join("Private").join("Nutrition");
    fs::create_dir_all(&dir).unwrap();
    let held = fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join(".lock")).unwrap();
    held.try_lock().expect("test holds the lock");
    let out = Command::new(env!("CARGO_BIN_EXE_pip-bot"))
        .env("DISCORD_INBOX_ALLOWED_IDS_FILE", valid_allowlist(&root))
        .env("PIP_NUTRITION_DIR", &dir)
        .output()
        .unwrap();
    assert!(!out.status.success(), "a second Pip must not run beside the first");
    drop(held);
}
