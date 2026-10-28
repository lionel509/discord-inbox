//! #130 unit 2 (edges): card numbers written with dots or zero-width gaps, a price right before a card, and
//! the pip-model helper's working directory. Invariant 2: `insight::mask_cards` never masks less than master.

use discord_inbox::{insight, redact};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn dotted_and_zero_width_cards_are_masked() {
    assert_eq!(redact::sensitive("4111.1111.1111.1111 chicken"), "****.****.****.1111 chicken");
    assert_eq!(
        redact::sensitive("4111\u{200B}1111\u{200B}1111\u{200B}1111 chicken"),
        "****\u{200B}****\u{200B}****\u{200B}1111 chicken"
    );
    assert_eq!(redact::sensitive("3782.822463.10005"), "****.******.*0005");
}

#[test]
fn price_before_a_card_still_masks_the_card() {
    assert_eq!(redact::sensitive("12.99 4111 1111 1111 1111"), "12.** **** **** **** 1111");
}

#[test]
fn dotted_numbers_that_are_not_cards_stay() {
    for s in ["12.99", "1.5 kg", "2026.10.27", "192.168.100.200"] {
        assert_eq!(redact::sensitive(s), s, "{s} must not be masked");
    }
}

/// `mask_cards` as it stands on e0bb735 (src/insight.rs), kept verbatim as the reference for invariant 2.
fn old_is_digit(c: char) -> bool {
    c.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&c)
}

fn old_mask_cards(text: &str) -> String {
    const MAX_GAP: usize = 16;
    let is_gap = |c: char| c.is_whitespace() || matches!(c, '-' | '\u{2013}' | '\u{2014}');
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if !old_is_digit(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        loop {
            if j < chars.len() && old_is_digit(chars[j]) {
                j += 1;
            } else if {
                let mut k = j;
                while k < chars.len() && k - j < MAX_GAP && is_gap(chars[k]) {
                    k += 1;
                }
                k > j && k < chars.len() && old_is_digit(chars[k])
            } {
                while !old_is_digit(chars[j]) {
                    j += 1;
                }
                j += 1;
            } else {
                break;
            }
        }
        let span = &chars[i..j];
        let digits = span.iter().filter(|&&c| old_is_digit(c)).count();
        if digits >= 12 {
            let mut seen = 0;
            for &c in span {
                if old_is_digit(c) {
                    seen += 1;
                    out.push(if seen + 4 <= digits { '*' } else { c });
                } else {
                    out.push(c);
                }
            }
        } else {
            out.extend(span);
        }
        i = j;
    }
    out
}

#[test]
fn property_never_masks_less_than_master() {
    const ODD: [char; 7] = [' ', '.', '-', '\u{200B}', '\u{A0}', 'x', '\n'];
    let mut x: u64 = 0x130_u64;
    let mut next = move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        x >> 33
    };
    for _ in 0..3000 {
        let len = 1 + (next() % 40) as usize;
        let s: String = (0..len)
            .map(|_| {
                if next() % 100 < 60 {
                    char::from(b'0' + (next() % 10) as u8)
                } else {
                    ODD[(next() % ODD.len() as u64) as usize]
                }
            })
            .collect();
        let old: Vec<char> = old_mask_cards(&s).chars().collect();
        let new_s = insight::mask_cards(&s);
        let new: Vec<char> = new_s.chars().collect();
        let src: Vec<char> = s.chars().collect();
        assert_eq!(new.len(), src.len(), "char count changed for {s:?}");
        for i in 0..src.len() {
            if old[i] == '*' {
                assert_eq!(new[i], '*', "master masks index {i} of {s:?} but the new rule shows it: {new_s:?}");
            }
            if new[i] != '*' {
                assert_eq!(new[i], src[i], "new rule changed a non-masked char in {s:?}");
            }
        }
    }
}

/// Runs `scripts/pip-model.sh` with a stub `claude` that prints its working directory.
fn helper_pwd(args: &[&str], name: &str) -> String {
    let root = std::env::temp_dir().join(format!("redact-r5-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let stub: PathBuf = root.join("claude-stub.sh");
    fs::write(&stub, "#!/bin/sh\npwd -P\ncat >/dev/null\n").unwrap();
    let mut perm = fs::metadata(&stub).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
    fs::set_permissions(&stub, perm).unwrap();
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/pip-model.sh");
    let out = Command::new("/bin/sh")
        .arg(script)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("DISCORD_INBOX_CLAUDE", &stub)
        .stdin(Stdio::null())
        .output()
        .expect("script runs");
    assert!(out.status.success(), "script failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn pip_model_runs_claude_from_the_photo_dir_or_tmp() {
    let root = std::env::temp_dir().join(format!("redact-r5-photo-{}", std::process::id()));
    let photos = root.join("photos");
    fs::create_dir_all(&photos).unwrap();
    let photo = photos.join("meal.jpg");
    fs::write(&photo, b"jpeg bytes").unwrap();
    let want = fs::canonicalize(&photos).unwrap().display().to_string();
    assert_eq!(helper_pwd(&[photo.to_str().unwrap()], "photo"), want);

    let got = helper_pwd(&[], "none");
    assert!(got == "/tmp" || got == "/private/tmp", "without a photo claude must run from /tmp, got {got:?}");
}

#[test]
fn dotted_card_beside_other_numbers_is_masked() {
    for s in [
        "4111.1111.1111.1111 5",
        "12.99 4111.1111.1111.1111",
        "4111.1111.1111.1111 12.99",
        "4111-1111.1111-1111 12.99",
        "4111\u{200B}1111.1111\u{200B}1111 12.99",
        "4111\u{200E}1111\u{200E}1111\u{200E}1111 chicken",
    ] {
        let o = redact::sensitive(s);
        assert!(!o.contains("4111"), "{s:?} leaked: {o:?}");
        assert_eq!(o.chars().count(), s.chars().count(), "{s:?} -> {o:?}");
    }
}
