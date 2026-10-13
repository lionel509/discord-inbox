use std::{env, path::PathBuf, process::Command};

/// Compiles the Vision OCR helper. A failed compile only turns OCR off; the build goes on.
fn main() {
    println!("cargo:rerun-if-changed=src/vision.swift");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("vision-ocr");
    let built = Command::new("xcrun")
        .args(["swiftc", "-O", "src/vision.swift", "-o"])
        .arg(&out)
        .status()
        .is_ok_and(|s| s.success());
    if !built {
        println!("cargo:warning=swiftc failed: OCR is off in this build");
    }
    println!("cargo:rustc-env=DISCORD_INBOX_VISION={}", out.display());
}
