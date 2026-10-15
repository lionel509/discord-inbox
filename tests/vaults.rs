//! Vault targets, the model's decision parser, and the note text helpers. Temp dirs and a made-up `Private` folder only.
use discord_inbox::vaults::{self, Target, Vaults, TARGETS};
use std::fs;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("di-vaults-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn no_disk() -> Vaults {
    Vaults::new(PathBuf::from("/nonexistent-vault-root"))
}

fn find(key: &str) -> &'static Target {
    TARGETS
        .iter()
        .find(|t| t.key == key)
        .unwrap_or_else(|| panic!("no target {key}"))
}

#[test]
fn target_matches_exact_and_case_insensitive_keys() {
    let v = no_disk();
    assert_eq!(v.target("Citadel/Ideas").map(|t| t.folder), Some("Citadel/Ideas"));
    assert_eq!(v.target("citadel/ideas").map(|t| t.folder), Some("Citadel/Ideas"));
    assert_eq!(v.target("blackrock").map(|t| t.folder), Some("BlackRock"));
    assert_eq!(v.target("  `State Street` ").map(|t| t.folder), Some("State Street"));
    assert_eq!(v.target("\"Mirae\"").map(|t| t.folder), Some("Mirae"));
}

#[test]
fn target_rejects_inbox_private_paths_dotdot_absolute_partial_empty() {
    let v = no_disk();
    for s in [
        "Inbox",
        "inbox",
        "Private",
        "Private/note.md",
        "../Private",
        "/tmp",
        "/Users/x",
        "Citadel/Ideas/../../Private",
        "Fidelity/Want to Make/../../Private",
        "Citadel",
        "Ideas",
        "Citadel/Ideas/",
        "",
        "   ",
    ] {
        assert!(v.target(s).is_none(), "accepted {s:?}");
    }
}

#[test]
fn targets_keys_equal_folders_no_dotdot_no_leading_slash() {
    assert_eq!(TARGETS.len(), 8);
    for t in TARGETS.iter() {
        assert_eq!(t.key, t.folder);
        assert!(!t.folder.contains(".."), "{}", t.folder);
        assert!(!t.folder.starts_with('/'), "{}", t.folder);
        assert!(!t.folder.contains("Private"), "{}", t.folder);
    }
}

const CANON: &str = "FOLDER: Citadel/Ideas\nTITLE: Solar powered fan idea\nTAGS: electronics, 3d-print\nREPLY:\nSounds like a project idea. Filed under Citadel.\n";

#[test]
fn parse_decision_canonical_reordered_missing_reply_junk_json() {
    let v = no_disk();
    let d = vaults::parse_decision(&v, CANON);
    assert_eq!(d.target.map(|t| t.key), Some("Citadel/Ideas"));
    assert_eq!(d.title, "Solar powered fan idea");
    assert_eq!(d.tags, vec!["electronics", "3d-print"]);
    assert_eq!(d.reply, "Sounds like a project idea. Filed under Citadel.");

    let reordered = "TAGS: electronics, 3d-print\nTITLE: Solar powered fan idea\nFOLDER: citadel/ideas\nREPLY:\nSounds like a project idea. Filed under Citadel.\n";
    let r = vaults::parse_decision(&v, reordered);
    assert_eq!(r.target.map(|t| t.key), Some("Citadel/Ideas"));
    assert_eq!(r.title, d.title);
    assert_eq!(r.reply, d.reply);

    let missing = "FOLDER: Fidelity/Want to Make\nTITLE: Lemon bars\nTAGS: recipe\n";
    let m = vaults::parse_decision(&v, missing);
    assert_eq!(m.target.map(|t| t.key), Some("Fidelity/Want to Make"));
    assert_eq!(m.reply, "Noted.");

    let junk = "lorem ipsum ".repeat(800);
    let j = vaults::parse_decision(&v, &junk);
    assert!(j.target.is_none());
    assert!(j.tags.is_empty());
    assert!(j.title.is_empty());
    assert!(!j.reply.is_empty());

    let json = r#"{"folder":"Citadel/Ideas","title":"x","reply":"hi"}"#;
    assert!(vaults::parse_decision(&v, json).target.is_none());
}

#[test]
fn clean_title_and_note_name() {
    let v = no_disk();
    let hostile = "TITLE: ../../etc/passwd\nFOLDER: Private\nREPLY:\nok";
    let d = vaults::parse_decision(&v, hostile);
    assert!(d.target.is_none());
    assert!(
        !d.title.contains('/') && !d.title.contains("..") && !d.title.contains('\n'),
        "{:?}",
        d.title
    );

    let long = format!("TITLE: {}\nREPLY:\nok", "word ".repeat(40));
    let d = vaults::parse_decision(&v, &long);
    assert!(d.title.chars().count() <= 60, "{}", d.title.chars().count());
    assert!(!d.title.is_empty() && !d.title.ends_with(' ') && !d.title.starts_with(' '));

    assert_eq!(vaults::note_name("2026-10-13T14:03:22Z", "Solar fan"), "2026-10-13 Solar fan.md");
    assert_eq!(vaults::note_name("2026-10-13T14:03:22Z", ""), "2026-10-13 Note 140322.md");
}

#[test]
fn clean_tags_and_front_extra_bare_yaml() {
    let v = no_disk();
    let d = vaults::parse_decision(
        &v,
        "FOLDER: State Street\nTAGS: Taxes, #Receipts, ../x, , taxes, extra, more\nREPLY:\nok",
    );
    assert!(d.tags.len() <= 3, "{:?}", d.tags);
    assert!(
        d.tags.iter().all(|t| !t.is_empty()
            && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')),
        "{:?}",
        d.tags
    );
    assert_eq!(d.tags[0], "taxes");
    let mut uniq = d.tags.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), d.tags.len(), "{:?}", d.tags);

    let t = find("State Street");
    let fx = vaults::front_extra(t, &d.tags);
    assert!(fx.starts_with("tags:\n"), "{fx:?}");
    assert!(fx.contains("  - finance\n"), "{fx:?}");
    assert!(fx.contains("  - taxes\n"), "{fx:?}");
    assert!(!fx.contains('#'), "{fx:?}");
    assert!(fx.lines().skip(1).all(|l| l.starts_with("  - ")), "{fx:?}");
    assert_eq!(vaults::hub_line(find("Citadel/Ideas")), "Hub: [[Ideas]]\n\n");
    assert_eq!(vaults::hub_line(t), "Hub: [[Home]]\n\n");
}

#[test]
fn dest_dir_none_for_missing_folder_and_symlink_escape() {
    let root = tmp("dest");
    let outside = tmp("outside");
    fs::create_dir_all(root.join("Private")).unwrap();
    fs::write(root.join("Private/note.md"), "private\n").unwrap();
    fs::create_dir_all(root.join("Citadel")).unwrap();
    fs::create_dir_all(root.join("State Street")).unwrap();
    let v = Vaults::new(root.clone());

    // A missing folder is refused, and nothing is created.
    assert!(v.dest_dir(find("Fidelity/Want to Make")).is_none());
    assert!(!root.join("Fidelity").exists());

    // A symlink into Private, or out of the root, is refused.
    std::os::unix::fs::symlink(root.join("Private"), root.join("Citadel/Ideas")).unwrap();
    assert!(v.dest_dir(find("Citadel/Ideas")).is_none());
    fs::remove_dir(root.join("State Street")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("State Street")).unwrap();
    assert!(v.dest_dir(find("State Street")).is_none());

    // A real folder is accepted.
    fs::create_dir_all(root.join("BlackRock")).unwrap();
    assert_eq!(
        v.dest_dir(find("BlackRock")).map(|d| d.ends_with("BlackRock")),
        Some(true)
    );
}

#[test]
fn prompt_is_bounded_and_lists_targets() {
    let sys = vaults::system_prompt();
    for t in TARGETS.iter() {
        assert!(sys.contains(t.key), "{}", t.key);
    }
    for label in ["Inbox", "FOLDER:", "TITLE:", "TAGS:", "REPLY:"] {
        assert!(sys.contains(label), "{label}");
    }
    let user = vaults::user_prompt("hi", &["x".repeat(20_000)]);
    assert!(user.chars().count() <= vaults::PROMPT_CHARS);
}

// ---- Shopping List, Answer and the bounded lookup bundle (#40). Temp dirs only. ----

#[test]
fn folder_answer_files_nothing() {
    let v = no_disk();
    let d = vaults::parse_decision(&v, "FOLDER: Answer\nTITLE: Costco list\nREPLY:\nHere it is.");
    assert!(d.answer);
    assert!(d.target.is_none());
    assert!(d.running.is_none());
    let plain = vaults::parse_decision(&v, "FOLDER: Citadel/Ideas\nREPLY:\nok");
    assert!(!plain.answer);
}

#[test]
fn folder_shopping_list_resolves_running_and_state_street_target() {
    let v = no_disk();
    let d = vaults::parse_decision(&v, "FOLDER: shopping list\nTITLE: Costco run\nREPLY:\nAdded.");
    assert_eq!(d.running.map(|r| r.key), Some("Shopping List"));
    assert_eq!(d.target.map(|t| t.key), Some("State Street"));
    assert!(!d.answer);
}

#[test]
fn running_table_is_one_shopping_list_under_state_street() {
    assert_eq!(vaults::RUNNING.len(), 1);
    let r = &vaults::RUNNING[0];
    assert_eq!(r.key, "Shopping List");
    assert!(r.path.starts_with("State Street/"), "{}", r.path);
    assert!(!r.path.contains(".."), "{}", r.path);
    assert_eq!(r.target_key, "State Street");
    assert!(no_disk().running("shopping list").is_some());
    assert!(no_disk().running("Citadel/Ideas").is_none());
}

#[test]
fn system_prompt_lists_shopping_list_and_answer() {
    let sys = vaults::system_prompt();
    assert!(sys.contains("Shopping List — "), "{sys}");
    assert!(sys.contains("Answer — "), "{sys}");
}

#[test]
fn answer_prompt_user_is_capped_at_lookup_chars() {
    let notes = vec![("a".to_string(), "x".repeat(20_000))];
    let (sys, user) = vaults::answer_prompt("q", &notes);
    assert!(!sys.is_empty());
    assert!(user.chars().count() <= vaults::LOOKUP_CHARS, "{}", user.chars().count());
    assert!(user.starts_with("q\n\nNotes:\n### a\n"));
}

#[test]
fn running_path_refuses_missing_and_symlink_accepts_regular_file() {
    let root = tmp("running");
    let outside = tmp("running-outside");
    fs::create_dir_all(root.join("State Street/Cash Flow/Spending")).unwrap();
    let v = Vaults::new(root.clone());
    let r = v.running("shopping list").expect("running entry");
    let p = root.join(r.path);

    // Missing: refused, and nothing is created.
    assert!(v.running_path(r).is_none());
    assert!(!p.exists());
    assert!(v.running_paths().is_empty());

    // A symlink to a file outside the vault: refused.
    fs::write(outside.join("real.md"), "x\n").unwrap();
    std::os::unix::fs::symlink(outside.join("real.md"), &p).unwrap();
    assert!(v.running_path(r).is_none());
    fs::remove_file(&p).unwrap();

    // A regular file at the path: accepted.
    fs::write(&p, "- [ ] Eggs\n").unwrap();
    assert_eq!(v.running_path(r), Some(p.clone()));
    assert_eq!(v.running_paths(), vec![p]);

    // The lookup allow-list covers every target folder and nothing else.
    assert_eq!(v.allowed_dirs().len(), TARGETS.len());
}

#[test]
fn parse_decision_reads_bolded_labels() {
    let v = no_disk();
    let d = vaults::parse_decision(&v, "**FOLDER:** Answer\n**REPLY:** hi");
    assert!(d.answer);
    assert_eq!(d.reply, "hi");
}

#[test]
fn parse_decision_strips_bold_from_folder_key() {
    let v = no_disk();
    let d = vaults::parse_decision(&v, "FOLDER: **State Street**\nREPLY:\nok");
    assert_eq!(d.target.map(|t| t.key), Some("State Street"));
}
