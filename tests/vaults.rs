//! Vault targets, the model's decision parser, and the note text helpers. Temp dirs and a made-up `Private` folder only.
use discord_inbox::vaults::{self, Target, Vaults, RUNNING, TARGETS};
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
    // Round 3: a trailing slash now matches its table key (norm drops "/"); it is still the static table entry, never a path.
    assert_eq!(v.target("Citadel/Ideas/").map(|t| t.key), Some("Citadel/Ideas"));
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

#[test]
fn folder_label_forms_all_route() {
    let v = no_disk();
    for input in [
        "FOLDER: Shopping List",
        "**FOLDER:** Shopping List",
        "__FOLDER:__ Shopping List",
        "- FOLDER: Shopping List",
        "FOLDER : Shopping List",
        "Folder: shopping list.",
        "FOLDER: **Shopping List**",
        "FOLDER: \"Shopping List\"",
    ] {
        let d = vaults::parse_decision(&v, input);
        assert_eq!(d.running.map(|r| r.key), Some("Shopping List"), "{input}");
    }
}

#[test]
fn folder_bang_todo_routes_with_or_without_bang() {
    let v = no_disk();
    for input in ["FOLDER: ! Todo", "FOLDER: Todo"] {
        let d = vaults::parse_decision(&v, input);
        assert_eq!(d.target.map(|t| t.key), Some("! Todo"), "{input}");
    }
}

#[test]
fn folder_path_keys_match_whole_and_partial_names_stay_none() {
    let v = no_disk();
    assert_eq!(vaults::parse_decision(&v, "FOLDER: Citadel/Ideas").target.map(|t| t.key), Some("Citadel/Ideas"));
    assert!(vaults::parse_decision(&v, "FOLDER: Citadel").target.is_none());
}

#[test]
fn folder_answer_with_period_and_bold_reply_label() {
    let v = no_disk();
    assert!(vaults::parse_decision(&v, "FOLDER: answer.").answer);
    let d = vaults::parse_decision(&v, "__REPLY:__ Added.");
    assert_eq!(d.reply, "Added.");
}

#[test]
fn loose_line_with_colon_stays_in_reply() {
    let v = no_disk();
    let d = vaults::parse_decision(&v, "Note to self: milk");
    assert_eq!(d.reply, "Note to self: milk");
}

#[test]
fn every_target_and_running_key_round_trips() {
    let v = no_disk();
    for t in TARGETS.iter() {
        assert_eq!(v.target(t.key).map(|x| x.key), Some(t.key));
    }
    for r in RUNNING.iter() {
        assert_eq!(v.running(r.key).map(|x| x.key), Some(r.key));
    }
}

#[test]
fn system_prompt_lists_answer_first_and_names_questions_without_mark() {
    let p = vaults::system_prompt();
    let a = p.find("Answer — ").expect("Answer line");
    assert!(a < p.find("Citadel/Ideas — ").unwrap());
    assert!(a < p.find("Shopping List — ").unwrap());
    assert!(p.contains("whats on my costco list"), "{p}");
}

// ---- #49: the agentic answer prompt names the tools and the vaults ----

#[test]
fn answer_prompt_names_tools_and_vaults() {
    let (system, user) = vaults::answer_prompt("what can i make with my cod", &[]);
    for want in ["search_notes", "read_note", "Fidelity", "Who Owes Me.md"] {
        assert!(system.contains(want), "{want}: {system}");
    }
    assert_eq!(user, "what can i make with my cod");
}

#[test]
fn system_prompt_calls_pantry_ideas_an_answer() {
    let sys = vaults::system_prompt();
    assert!(sys.contains("a request for ideas or advice from his notes is also Answer"), "{sys}");
}

#[test]
fn answer_prompt_names_on_hand_staples_and_house_rules() {
    let (system, _) = vaults::answer_prompt("what can I make", &[]);
    assert!(system.contains(vaults::PANTRY), "{system}");
    for s in vaults::STAPLES {
        assert!(system.contains(s), "{system}");
    }
    assert!(system.contains("alcohol"), "{system}");
    assert!(system.contains("caffeinated"), "{system}");
    assert!(system.contains("for one"), "{system}");
}

#[test]
fn chat_key_files_nothing() {
    let v = no_disk();
    let d = vaults::parse_decision(&v, "FOLDER: Chat\nTITLE: x\nTAGS: a\nREPLY:\nOh nice");
    assert!(d.target.is_none());
    assert!(d.running.is_none());
    assert!(!d.answer);
    assert_eq!(d.reply, "Oh nice");
}

#[test]
fn system_prompt_names_chat_and_window() {
    let sys = vaults::system_prompt();
    for want in ["Chat — ", "which one should i get", "Now he says:", "never say that you saved"] {
        assert!(sys.contains(want), "{want}: {sys}");
    }
    assert!(!sys.contains("say what it is"), "{sys}");
    let (system, _) = vaults::answer_prompt("q", &[]);
    assert!(system.contains("Now he says:"), "{system}");
}

#[test]
fn with_window_formats_turns() {
    assert_eq!(vaults::with_window(&[], "q"), "q");
    let turns = vec![("a".to_string(), "b".to_string()), ("c".to_string(), "d".to_string())];
    assert_eq!(
        vaults::with_window(&turns, "q"),
        "Earlier in this chat, oldest first:\nLionel: a\nStormy: b\nLionel: c\nStormy: d\n\nNow he says:\nq"
    );
}

/// The prompts name attached screenshot text and the no-promise rule (#73).
#[test]
fn prompts_name_attached_text_and_the_no_promise_rule() {
    let (system, _) = vaults::answer_prompt("q", &[]);
    assert!(system.contains("Attached text:"), "{system}");
    let filing = vaults::system_prompt();
    assert!(filing.contains("never promise to check"), "{filing}");
    assert!(filing.contains("how much does William owe me"), "{filing}");
}

/// A question ends in "?" even with a trailing "!", emoji or spaces; plain text does not (#73).
#[test]
fn is_question_sees_through_trailing_punctuation() {
    for q in ["ok?", "ok? ", "really?!", "ok?? 😺"] {
        assert!(vaults::is_question(q), "{q}");
    }
    for s in ["", "what 😺", "how much does willam owe me"] {
        assert!(!vaults::is_question(s), "{s}");
    }
}

/// Only a reply that promises a lookup counts; a plain answer or a reaction does not (#73).
#[test]
fn promised_matches_lookup_promises_only() {
    for r in ["Let me dig through your notes… give me a sec", "I’ll check the ledger", "One sec, lemme look"] {
        assert!(vaults::promised(r), "{r}");
    }
    for r in ["William owes you $12.00.", "Nice, the Fresh is the better pick 🐾", "Noted."] {
        assert!(!vaults::promised(r), "{r}");
    }
}

/// A fullwidth question mark (Chinese or Japanese IME) ends a question too (#75).
#[test]
fn is_question_counts_fullwidth_question_mark() {
    for q in ["你要吗？", "ok？ 😺"] {
        assert!(vaults::is_question(q), "{q}");
    }
}

/// Lookup promises phrased with search or see are caught too (#75).
#[test]
fn promised_catches_search_and_see_promises() {
    for r in ["Let me search your notes for that", "I'll search through your notes", "Let me see what your notes say", "I'll look in your notes"] {
        assert!(vaults::promised(r), "{r}");
    }
}

/// Words that only contain a promise phrase, and "look forward" / "check in" / "check out", are not promises (#75).
#[test]
fn promised_ignores_words_that_only_contain_a_phrase() {
    for r in ["Oh, someone secretly bought you that gift", "I'll checkout the new menu", "I'll look forward to it", "I'll check in on you tomorrow", "I'll check out the menu"] {
        assert!(!vaults::promised(r), "{r}");
    }
}

/// A question keeps its whole text up to the limit the caller asks for (#75).
#[test]
fn user_prompt_within_keeps_a_long_question() {
    let long = "q".repeat(5000);
    assert_eq!(vaults::user_prompt_within(&long, &[], vaults::LOOKUP_CHARS).chars().count(), 5000);
    assert_eq!(vaults::user_prompt(&long, &[]).chars().count(), vaults::PROMPT_CHARS);
}
