//! The read-only lookup tools (#49): roots, search, read, list and the privacy invariant. No network.
//! Fixture folders and the `SECRET-PRIVATE-TOKEN` sentinel only; `Private` is a made-up stand-in.
use discord_inbox::tools::{Lookup, MAX_FILE_BYTES, READ_CHARS, ROOTS};
use std::fs;
use std::path::{Path, PathBuf};

const SECRET: &str = "SECRET-PRIVATE-TOKEN";
const REFUSED: &str = "not found or not allowed";

fn put(p: &Path, body: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// A fresh vault `(root, inbox)`: every root folder except `Bridgewater`, which is a symlink to `Private`.
fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("di-tools-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let root = base.join("vault");
    let inbox = base.join("Inbox");
    for r in ROOTS.iter().filter(|r| **r != "Bridgewater") {
        fs::create_dir_all(root.join(r)).unwrap();
    }
    put(&root.join("Private/secret.md"), SECRET);
    put(&root.join("Fidelity/Pantry/Pantry.md"), "cod fillets, soy sauce, gochujang, rice\n");
    put(&root.join("Fidelity/Pantry/Korean Shelf.md"), "gochujang and doenjang\n");
    put(&root.join("State Street/Assets/Receivables/Who Owes Me.md"), "| William | $12.00 |\n");
    put(&root.join("Fidelity/.trash/old.md"), SECRET);
    put(&root.join("Fidelity/.obsidian/x.md"), SECRET);
    put(&root.join("BlackRock/.git/x.md"), SECRET);
    put(&inbox.join(".filed"), SECRET);
    put(&root.join("Fidelity/pic.png"), SECRET);
    put(&root.join("Fidelity/big.md"), &format!("{SECRET}{}", "x".repeat(MAX_FILE_BYTES as usize)));
    put(&root.join("Citadel/plain.md"), "tomato basil\n");
    std::os::unix::fs::symlink("../Private", root.join("Fidelity/Shelf")).unwrap();
    std::os::unix::fs::symlink("../Private/secret.md", root.join("Citadel/link.md")).unwrap();
    std::os::unix::fs::symlink("Private", root.join("Bridgewater")).unwrap();
    (root, inbox)
}

fn lookup(name: &str) -> Lookup {
    let (root, inbox) = fixture(name);
    Lookup::new(root, inbox)
}

#[test]
fn roots_lists_nine_plus_inbox_and_drops_symlinked_root() {
    let names: Vec<String> = lookup("roots").roots().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names.len(), 9, "{names:?}");
    assert!(names.contains(&"Inbox".to_string()), "{names:?}");
    assert!(names.contains(&"Fidelity".to_string()), "{names:?}");
    assert!(!names.contains(&"Bridgewater".to_string()), "{names:?}");
    assert!(!names.contains(&"Private".to_string()), "{names:?}");
}

#[test]
fn search_finds_pantry_by_filename_and_content() {
    let hits = lookup("search").search_notes("pantry cod", None).unwrap();
    assert_eq!(hits.lines().next().unwrap(), "Fidelity/Pantry/Pantry.md", "{hits}");
    assert!(hits.contains("cod"), "{hits}");
}

#[test]
fn search_with_folder_scope() {
    let lk = lookup("scope");
    let hits = lk.search_notes("gochujang", Some("Fidelity/Pantry")).unwrap();
    assert!(hits.contains("Fidelity/Pantry/Pantry.md"), "{hits}");
    assert_eq!(lk.search_notes("gochujang", Some("BlackRock")).unwrap(), "No notes matched.");
    assert!(lk.search_notes("gochujang", Some("Private")).is_err());
}

#[test]
fn search_skips_hidden_binary_oversize_and_symlinked() {
    let lk = lookup("skips");
    assert_eq!(lk.search_notes(SECRET, None).unwrap(), "No notes matched.");
}

#[test]
fn read_note_returns_pantry_text_and_cuts_at_read_chars() {
    let (root, inbox) = fixture("read");
    put(&root.join("Fidelity/Long.md"), &"a".repeat(READ_CHARS + 500));
    let lk = Lookup::new(root, inbox);
    let t = lk.read_note("Fidelity/Pantry/Pantry.md").unwrap();
    assert!(t.contains("cod fillets"), "{t}");
    let long = lk.read_note("Fidelity/Long.md").unwrap();
    assert!(long.contains("[cut at 6000 chars]"), "{long}");
    assert!(long.chars().count() <= READ_CHARS + 40, "{}", long.chars().count());
}

#[test]
fn read_note_refuses_private_by_name_dotdot_absolute_symlinked_dir_symlinked_file() {
    let (root, inbox) = fixture("refuse");
    let abs = format!("{}/Private/secret.md", root.display());
    let lk = Lookup::new(root, inbox);
    for bad in [
        "Private/secret.md",
        "Fidelity/../Private/secret.md",
        "Fidelity/Pantry/../../Private/secret.md",
        abs.as_str(),
        "Fidelity/Shelf/secret.md",
        "Citadel/link.md",
        "Bridgewater/secret.md",
    ] {
        assert_eq!(lk.read_note(bad).unwrap_err(), REFUSED, "{bad}");
    }
}

#[test]
fn read_note_refuses_hidden_components() {
    let lk = lookup("hidden");
    for bad in ["Fidelity/.trash/old.md", "Fidelity/.obsidian/x.md", "BlackRock/.git/x.md", "Inbox/.filed", "Fidelity/pic.png"] {
        assert_eq!(lk.read_note(bad).unwrap_err(), REFUSED, "{bad}");
    }
}

#[test]
fn list_folder_root_shows_only_allowed_roots() {
    let out = lookup("list-root").list_folder("").unwrap();
    let names: Vec<&str> = out.lines().collect();
    assert!(names.contains(&"Fidelity"), "{out}");
    assert!(names.contains(&"Inbox"), "{out}");
    assert!(!names.iter().any(|n| n.contains("Private") || n.contains("Bridgewater")), "{out}");
}

#[test]
fn list_folder_skips_hidden_and_symlinks() {
    let out = lookup("list-skip").list_folder("Fidelity").unwrap();
    assert!(out.lines().any(|l| l == "Pantry/"), "{out}");
    assert!(!out.contains("Shelf"), "{out}");
    assert!(!out.contains(".trash"), "{out}");
    assert!(!out.contains(".obsidian"), "{out}");
}

#[test]
fn call_dispatches_and_rejects_unknown_tool_and_bad_json() {
    let lk = lookup("call");
    assert!(lk.call("search_notes", r#"{"query":"gochujang"}"#).unwrap().contains("Pantry.md"));
    assert!(lk.call("read_note", r#"{"path":"Fidelity/Pantry/Pantry.md"}"#).unwrap().contains("cod fillets"));
    assert!(lk.call("list_folder", "{}").is_ok());
    assert!(lk.call("delete_note", "{}").is_err());
    assert!(lk.call("search_notes", "not json").is_err());
}

#[test]
fn tool_output_is_redacted() {
    let (root, inbox) = fixture("redact");
    put(&root.join("Fidelity/Pantry/card.md"), "card 4111 1111 1111 1111 for groceries");
    let lk = Lookup::new(root, inbox);
    let out = lk.read_note("Fidelity/Pantry/card.md").unwrap();
    assert!(!out.contains("4111 1111"), "{out}");
}

/// Random paths built from hostile fragments: no tool output may ever carry bytes from the private stand-in.
#[test]
fn no_tool_output_ever_leaks_the_sentinel() {
    let (root, inbox) = fixture("invariant");
    let lk = Lookup::new(root.clone(), inbox);
    let frags = [
        "Fidelity", "Pantry", "Pantry.md", "Shelf", "secret.md", "Private", "..", ".", ".trash", ".obsidian", ".git",
        "Inbox", ".filed", "link.md", "Bridgewater", "/", "\\", "",
    ];
    let abs = root.display().to_string();
    let mut seed: u64 = 49;
    let mut next = move |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n
    };
    for i in 0..2000 {
        let k = 1 + next(4);
        let mut parts: Vec<&str> = Vec::new();
        for _ in 0..k {
            parts.push(frags[next(frags.len())]);
        }
        let mut p = parts.join("/");
        if i % 5 == 0 {
            p = format!("{abs}/{p}");
        }
        let outs = [lk.read_note(&p), lk.list_folder(&p), lk.search_notes(&p, None)];
        for out in outs {
            let text = match out {
                Ok(s) | Err(s) => s,
            };
            assert!(!text.contains(SECRET), "leak via {p:?}: {text}");
        }
    }
    assert_eq!(lk.search_notes(SECRET, None).unwrap(), "No notes matched.");
}

// Regression (PR #51 review round 1): models copy the trailing slash from list_folder output.
#[test]
fn trailing_slash_on_a_folder_path_is_accepted() {
    let base = std::env::temp_dir().join(format!("di-trailing-slash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("vault");
    std::fs::create_dir_all(root.join("Fidelity/Pantry")).unwrap();
    std::fs::write(root.join("Fidelity/Pantry/Pantry.md"), "cod fillets\n").unwrap();
    let lk = discord_inbox::tools::Lookup::new(root, base.join("Inbox"));
    let listing = lk.list_folder("Fidelity/").expect("trailing slash listed");
    assert!(listing.contains("Pantry/"));
    let hits = lk.search_notes("cod", Some("Fidelity/Pantry/")).expect("trailing slash search");
    assert!(hits.contains("Pantry.md"));
    assert!(lk.list_folder("../Fidelity/").is_err(), "parent traversal still refused");
}

// Regression (PR #51 review round 1): a one-character CJK query is kept (the plan's 2-byte floor).
#[test]
fn single_cjk_character_query_is_kept() {
    let base = std::env::temp_dir().join(format!("di-cjk-one-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("vault");
    std::fs::create_dir_all(root.join("Mirae")).unwrap();
    std::fs::write(root.join("Mirae/学习.md"), "学习 汉字\n").unwrap();
    let lk = discord_inbox::tools::Lookup::new(root, base.join("Inbox"));
    let hits = lk.search_notes("学", None).expect("one-character query");
    assert!(hits.contains("学习.md"), "hits = {hits}");
}
