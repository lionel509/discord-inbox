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

#[test]
fn demo_script_refuse_check_honours_cargo_target_dir() {
    // With CARGO_TARGET_DIR elsewhere, the hardcoded target/debug path is missing.
    // Before the fix, step 2 reported exit 127 and the script still exited 0.
    let dir = scratch("target-dir");
    let target = std::env::temp_dir().join("di-demo-test-target");
    let out = run_demo(None, Some(&target), &dir);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("exit code: 1") && !stdout.contains("No such file"),
        "step 2 did not show refuse-to-start; script status {:?}\n{stdout}",
        out.status.code()
    );
    assert!(out.status.success(), "script failed:\n{stdout}");
}
