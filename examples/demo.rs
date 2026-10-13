//! Offline demo of the allowlist gate and the Inbox writer. No Discord, no token.
//! Run with `sh examples/demo.sh` (also runs the refuse-to-start check).
//!
//! The allowlist is loaded through `allowlist::default_path()`, the same path the
//! bot reads at startup. Messages go through `dump::admit_and_write`, the same gate
//! and write the bot's `ingest` calls. The only part not run is the serenity
//! event plumbing and the pre-download check in `main.rs`, which needs a live gateway.
//!
//! The IDs below are stand-ins. The real IDs live only in the local config file.
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

fn main() {
    let tmp = std::env::temp_dir().join(format!("discord-inbox-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let tmp_text = tmp.display().to_string();
    let inbox = tmp.join("Inbox");

    // Config file in the temp dir, read through the bot's real config path.
    let cfg = tmp.join("allowed_ids");
    fs::write(
        &cfg,
        format!("# stand-in IDs for the demo\n{STAND_IN_MAIN}\n{STAND_IN_ALT}\n"),
    )
    .unwrap();
    // Edition 2021: set_var is safe. Only this process reads it.
    std::env::set_var("DISCORD_INBOX_ALLOWED_IDS_FILE", &cfg);
    let path = allowlist::default_path();
    let allow: Allowlist = Allowlist::load(&path).expect("demo allowlist loads");
    println!(
        "allowlist loaded from {}: 2 IDs (main, alt)",
        show(&path, &tmp_text)
    );
    println!();

    let messages = vec![
        (
            "stand-in for main",
            Incoming {
                msg_id: 1427000000000000001,
                author_id: STAND_IN_MAIN,
                received: "2026-10-13T14:03:22Z".into(),
                content: "https://example.com/recipe".into(),
                attachments: vec![],
            },
        ),
        (
            "stand-in for alt",
            Incoming {
                msg_id: 1427000000000000002,
                author_id: STAND_IN_ALT,
                received: "2026-10-13T14:05:10Z".into(),
                content: "see attached".into(),
                attachments: vec![("note.txt".into(), b"hello".to_vec())],
            },
        ),
        (
            "third ID",
            Incoming {
                msg_id: 1427000000000000003,
                author_id: THIRD_ID,
                received: "2026-10-13T14:06:00Z".into(),
                content: "spam".into(),
                attachments: vec![],
            },
        ),
    ];

    for (label, m) in &messages {
        match admit_and_write(&allow, &inbox, m).expect("write") {
            Some(note) => println!(
                "ACCEPTED {} ({}): wrote {}",
                m.author_id,
                label,
                note.file_name().unwrap().to_string_lossy()
            ),
            None => println!(
                "IGNORED {} ({}): not on allowlist, nothing written",
                m.author_id, label
            ),
        }
    }

    let names: Vec<String> = fs::read_dir(&inbox)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let notes = names.iter().filter(|n| n.ends_with(".md")).count();
    let files = names.len() - notes;
    println!();
    println!(
        "Inbox {} now holds {notes} notes and {files} attachment; nothing was written for the ignored sender.",
        show(&inbox, &tmp_text)
    );

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
