//! Offline demo: runs the allowlist gate and the dump writer on fake messages.
//! No Discord, no token. Run with `cargo run --example demo`.
use discord_inbox::allowlist::parse;
use discord_inbox::dump::{write_note, Incoming};
use std::fs;

fn main() {
    let dir = std::env::temp_dir().join(format!("discord-inbox-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let inbox = dir.join("Inbox");
    fs::create_dir_all(&dir).unwrap();

    let allow_text = "# demo allowlist (fake IDs)\n100000000000000001\n100000000000000002\n";
    let allow = parse(allow_text).expect("demo allowlist parses");

    let messages = vec![
        Incoming {
            msg_id: 1427000000000000001,
            author_id: 100000000000000001,
            received: "2026-10-13T14:03:22Z".into(),
            content: "https://example.com/recipe".into(),
            attachments: vec![],
        },
        Incoming {
            msg_id: 1427000000000000002,
            author_id: 100000000000000002,
            received: "2026-10-13T14:05:10Z".into(),
            content: "see attached".into(),
            attachments: vec![("note.txt".into(), b"hello".to_vec())],
        },
        Incoming {
            msg_id: 1427000000000000003,
            author_id: 999999999999999999,
            received: "2026-10-13T14:06:00Z".into(),
            content: "spam".into(),
            attachments: vec![],
        },
    ];

    for m in &messages {
        if allow.contains(m.author_id) {
            let note = write_note(&inbox, m).unwrap();
            println!(
                "dumped {} -> {}",
                m.author_id,
                note.file_name().unwrap().to_string_lossy()
            );
        } else {
            println!("ignored {}", m.author_id);
        }
    }

    println!("\n== {} ==", inbox.display());
    let mut names: Vec<_> = fs::read_dir(&inbox)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for n in &names {
        println!("  {n}");
    }
    for n in names.iter().filter(|n| n.ends_with(".md")) {
        println!("\n== {n} ==");
        print!("{}", fs::read_to_string(inbox.join(n)).unwrap());
    }
    fs::remove_dir_all(&dir).unwrap();
}
