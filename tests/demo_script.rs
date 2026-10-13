//! Regression tests for `examples/demo.sh`. Each one runs the real script; a
//! `cargo` shim on PATH stands in for the failure under test and hands every
//! other cargo call to the real cargo.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn real_cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("di-demo-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a `cargo` shim whose body runs first. `$1` and `$*` are the cargo args.
fn shim_dir(dir: &Path, body: &str) -> PathBuf {
    let bin = dir.join("shim-bin");
    fs::create_dir_all(&bin).unwrap();
    let cargo = bin.join("cargo");
    let text = format!(
        "#!/bin/sh\n{body}\nexec \"{}\" \"$@\"\n",
        real_cargo()
    );
    fs::write(&cargo, text).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn run_demo(path_prefix: Option<&Path>, target_dir: Option<&Path>, tmp: &Path) -> Output {
    let mut cmd = Command::new("sh");
    cmd.arg("examples/demo.sh")
        .current_dir(repo_root())
        .env("TMPDIR", tmp);
    if let Some(bin) = path_prefix {
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{path}", bin.display()));
    }
    if let Some(t) = target_dir {
        cmd.env("CARGO_TARGET_DIR", t);
    }
    cmd.output().unwrap()
}

#[test]
fn demo_script_fails_when_the_allowlist_demo_fails() {
    // The demo's `cargo run --example demo` exits 101. Before the fix, the
    // `| sed` pipeline hid that and the script exited 0.
    let dir = scratch("demo-fails");
    let bin = shim_dir(
        &dir,
        r#"case "$*" in *run*--example*) echo "shim: demo refused" >&2; exit 101;; esac"#,
    );
    let out = run_demo(Some(&bin), None, &dir);
    assert!(
        !out.status.success(),
        "demo.sh exited {:?} although the allowlist demo failed\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn demo_script_fails_when_refuse_to_start_does_not_exit_1() {
    // The bot starts instead of refusing: `cargo run --bin` exits 0 with no output.
    let dir = scratch("no-refuse");
    let bin = shim_dir(
        &dir,
        r#"case "$*" in *run*--bin*) exit 0;; esac"#,
    );
    let out = run_demo(Some(&bin), None, &dir);
    assert!(
        !out.status.success(),
        "demo.sh exited {:?} although step 2 did not refuse to start\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Copies only what demo.sh needs. No `target/`, so nothing prebuilt is in reach.
fn copy_crate(dest: &Path) -> PathBuf {
    let copy = dest.join("crate");
    fs::create_dir_all(&copy).unwrap();
    for item in ["Cargo.toml", "Cargo.lock", "src", "examples"] {
        let ok = Command::new("cp")
            .args(["-R", repo_root().join(item).to_str().unwrap(), copy.to_str().unwrap()])
            .status()
            .unwrap()
            .success();
        assert!(ok, "copy {item}");
    }
    copy
}

fn real_target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
}

fn run_demo_in(crate_dir: &Path, tmp: &Path) -> Output {
    run_demo_with_target(crate_dir, tmp, &real_target_dir())
}

fn run_demo_with_target(crate_dir: &Path, tmp: &Path, target: &Path) -> Output {
    Command::new("sh")
        .arg("examples/demo.sh")
        .current_dir(crate_dir)
        .env("TMPDIR", tmp)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap()
}

#[test]
fn demo_script_refuse_check_honours_cargo_target_dir() {
    // The copy has no target/, so a hardcoded target/debug/discord-inbox path is
    // missing even under a bare `cargo test`. Step 2 would then exit 127.
    let dir = scratch("target-dir");
    let copy = copy_crate(&dir);
    assert!(!copy.join("target").exists());
    let out = run_demo_in(&copy, &dir);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Exact line match: "exit code: 127" also starts with "exit code: 1".
    let refused = stdout.lines().any(|l| l == "exit code: 1");
    assert!(
        refused && stdout.contains("allowed_ids not found"),
        "step 2 did not show refuse-to-start; script status {:?}\n{stdout}",
        out.status.code()
    );
    assert!(out.status.success(), "script failed:\n{stdout}");
}

#[test]
fn demo_script_fails_when_the_gate_admits_a_stranger() {
    // Break the gate in a copy: Allowlist::contains always says yes.
    let dir = scratch("broken-gate");
    let copy = copy_crate(&dir);
    let file = copy.join("src/allowlist.rs");
    let text = fs::read_to_string(&file).unwrap();
    let broken = text.replace("self.0.contains(&user_id)", "{ let _ = user_id; true }");
    assert_ne!(text, broken, "patch did not apply");
    fs::write(&file, broken).unwrap();
    // Own target dir: the broken build must not overwrite the real demo binary
    // that the other tests run in parallel. Kept across runs so deps build once.
    let target = std::env::temp_dir().join("di-demo-broken-gate-target");
    let out = run_demo_with_target(&copy, &dir, &target);
    assert!(
        !out.status.success(),
        "demo.sh exited 0 with a gate that admits everyone\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn demo_script_survives_a_hash_in_tmpdir() {
    let dir = scratch("hash#dir");
    let out = run_demo(None, None, &dir);
    assert!(
        out.status.success(),
        "demo.sh failed with # in TMPDIR\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
