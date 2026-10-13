//! Offline demo of the allowlist gate and the Inbox writer. No Discord, no token.
//! Run with `sh examples/demo.sh` (also runs the refuse-to-start check).
//!
//! The allowlist is written to a temp file and loaded through `allowlist::default_path()`
//! (via DISCORD_INBOX_ALLOWED_IDS_FILE), the same path the bot reads at startup.
//! Messages go through `dump::admit_and_write`, the same gate and write that `ingest`
//! in main.rs calls. NOT exercised here: the serenity event plumbing and the
//! pre-download check in `ingest`, which need a live gateway.
//!
//! Real IDs come from the environment (DEMO_MAIN_ID, DEMO_ALT_ID) and are never printed.
//! Without them the demo uses stand-in IDs and says so.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::dump::{admit_and_write, sha256_hex, Incoming};
use std::fs;
use std::path::Path;

const STAND_IN_MAIN: u64 = 100000000000000001;
const STAND_IN_ALT: u64 = 100000000000000002;
const THIRD_ID: u64 = 999999999999999999;

fn show(path: &Path, tmp: &str) -> String {
    path.display().to_string().replace(tmp, "<tmp>")
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn id_from_env(var: &str, fallback: u64) -> (u64, bool) {
    match std::env::var(var) {
        Ok(v) => (v.trim().parse().expect("DEMO_*_ID must be a number"), true),
        Err(_) => (fallback, false),
    }
}

fn main() {
    let (main_id, real) = id_from_env("DEMO_MAIN_ID", STAND_IN_MAIN);
    let (alt_id, _) = id_from_env("DEMO_ALT_ID", STAND_IN_ALT);
    let source = if real { "" } else { " (stand-in IDs)" };

    let tmp = std::env::temp_dir().join(format!("discord-inbox-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let tmp_text = tmp.display().to_string();
    let inbox = tmp.join("Inbox");

    // Config file in the temp dir, read through the bot's real config path.
    let cfg = tmp.join("allowed_ids");
    fs::write(&cfg, format!("# demo allowlist\n{main_id}\n{alt_id}\n")).unwrap();
    // Edition 2021: set_var is safe. Only this process reads it.
    std::env::set_var("DISCORD_INBOX_ALLOWED_IDS_FILE", &cfg);
    let path = allowlist::default_path();
    let allow: Allowlist = Allowlist::load(&path).expect("demo allowlist loads");
    println!(
        "allowlist loaded from {}: 2 IDs (main, alt){source}",
        show(&path, &tmp_text)
    );
    println!();

    let messages = vec![
        (
            "main",
            Incoming {
                msg_id: 1427000000000000001,
                author_id: main_id,
                received: "2026-10-13T14:03:22Z".into(),
                content: "https://example.com/recipe".into(),
                attachments: vec![],
            },
        ),
        (
            "alt",
            Incoming {
                msg_id: 1427000000000000002,
                author_id: alt_id,
                received: "2026-10-13T14:05:10Z".into(),
                content: "see attached".into(),
                attachments: vec![("note.txt".into(), b"hello".to_vec())],
            },
        ),
        (
            "third",
            Incoming {
                msg_id: 1427000000000000003,
                author_id: THIRD_ID,
                received: "2026-10-13T14:06:00Z".into(),
                content: "spam".into(),
                attachments: vec![],
            },
        ),
    ];

    let mut alt_note = None;
    for (label, m) in &messages {
        match admit_and_write(&allow, &inbox, m).expect("write") {
            Some(note) => {
                println!(
                    "ACCEPTED {label}: wrote {}",
                    note.file_name().unwrap().to_string_lossy()
                );
                if m.author_id == alt_id {
                    alt_note = Some(note);
                }
            }
            None => println!(
                "IGNORED {}: not on allowlist, nothing written",
                m.author_id
            ),
        }
    }

    // The alt note carries the attachment. Show it whole, with the real IDs masked.
    if let Some(note) = alt_note {
        let mut pairs = [(main_id, "main"), (alt_id, "alt")];
        // Longest first, so a short ID cannot mask part of a longer one.
        pairs.sort_by_key(|(id, _)| std::cmp::Reverse(id.to_string().len()));
        let mut text = fs::read_to_string(&note).unwrap();
        for (id, label) in pairs {
            text = text.replace(&id.to_string(), label);
        }
        println!();
        println!("--- note for alt (real IDs masked) ---");
        print!("{text}");
        println!("--- end of note ---");
    }

    let names: Vec<String> = fs::read_dir(&inbox)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let notes = names.iter().filter(|n| n.ends_with(".md")).count();
    let files = names.len() - notes;
    println!();
    println!(
        "Inbox {} now holds {} and {}; nothing was written for the ignored sender.",
        show(&inbox, &tmp_text),
        plural(notes, "note"),
        plural(files, "attachment")
    );
    println!("(The real target is ~/Documents/Inbox/.)");

    for n in names.iter().filter(|n| !n.ends_with(".md")) {
        let bytes = fs::read(inbox.join(n)).unwrap();
        println!(
            "attachment {n}: {:?}  sha256 {}",
            String::from_utf8_lossy(&bytes),
            sha256_hex(&bytes)
        );
    }

    fs::remove_dir_all(&tmp).unwrap();
}
