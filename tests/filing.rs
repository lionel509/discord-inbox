//! Moving a note and its attachments into a vault folder, and the `.filed` ledger. Temp dirs only.
use discord_inbox::dump::{append_running, file_away, filed_to, lookup_notes, write_note, Incoming};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Returns (Inbox, vault root). The root has a `Citadel/Ideas` folder; the Inbox is created.
fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("di-filing-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    fs::create_dir_all(root.join("Citadel/Ideas")).unwrap();
    (inbox, root)
}

fn msg(id: u64) -> Incoming {
    Incoming {
        msg_id: id,
        author_id: 1,
        received: "2026-10-13T14:03:22Z".into(),
        content: "sketch of a solar fan".into(),
        attachments: vec![("sketch.txt".into(), Some(b"sketch".to_vec()))],
        ocr: vec![],
    }
}

const NAME: &str = "2026-10-13 Solar fan.md";
const FX: &str = "tags:\n  - citadel\n  - discord-inbox\n";
const LEAD: &str = "Hub: [[Ideas]]\n\n";

/// Files in `dir` whose name ends with `suffix`.
fn named(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().ends_with(suffix))
        .collect()
}

#[test]
fn file_away_moves_note_and_attachment_and_records_ledger() {
    let (inbox, root) = scratch("moves");
    let m = msg(500);
    let note = write_note(&inbox, &m).unwrap();
    let att = named(&inbox, "_sketch.txt");
    assert_eq!(att.len(), 1);
    let dest_dir = root.join("Citadel/Ideas");
    let dest = file_away(&inbox, &m, &dest_dir, NAME, FX, LEAD).unwrap();
    assert_eq!(dest, dest_dir.join(NAME));

    let text = fs::read_to_string(&dest).unwrap();
    assert!(text.contains("lane: dump\ntags:\n  - citadel\n"), "{text}");
    assert!(text.contains("Hub: [[Ideas]]\n\n"), "{text}");
    assert!(text.contains("sketch of a solar fan"), "{text}");

    assert!(!note.exists(), "note still in Inbox");
    assert!(!att[0].exists(), "attachment still in Inbox");
    let moved = named(&dest_dir, "_sketch.txt");
    assert_eq!(moved.len(), 1);
    assert_eq!(fs::read(&moved[0]).unwrap(), b"sketch");
    assert!(named(&dest_dir, ".tmp").is_empty(), "tmp file left behind");
    assert_eq!(filed_to(&inbox, 500), Some(dest.display().to_string()));
}

#[test]
fn file_away_never_overwrites_note() {
    let (inbox, root) = scratch("note-clash");
    let m = msg(501);
    let note = write_note(&inbox, &m).unwrap();
    let dest_dir = root.join("Citadel/Ideas");
    let dest = dest_dir.join(NAME);
    fs::write(&dest, "OLD\n").unwrap();
    let err = file_away(&inbox, &m, &dest_dir, NAME, FX, LEAD).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "OLD\n");
    assert!(note.is_file(), "note left Inbox");
    assert_eq!(named(&inbox, "_sketch.txt").len(), 1);
    assert!(named(&dest_dir, "_sketch.txt").is_empty(), "attachment link left behind");
    assert!(named(&dest_dir, ".tmp").is_empty(), "tmp file left behind");
    assert_eq!(filed_to(&inbox, 501), None);
}

#[test]
fn file_away_never_overwrites_attachment() {
    let (inbox, root) = scratch("att-clash");
    let m = msg(502);
    let note = write_note(&inbox, &m).unwrap();
    let att = named(&inbox, "_sketch.txt");
    let att_name = att[0].file_name().unwrap().to_owned();
    let dest_dir = root.join("Citadel/Ideas");
    fs::write(dest_dir.join(&att_name), "OLD").unwrap();
    let err = file_away(&inbox, &m, &dest_dir, NAME, FX, LEAD).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read(dest_dir.join(&att_name)).unwrap(), b"OLD");
    assert!(!dest_dir.join(NAME).exists(), "note written despite a clashing attachment");
    assert!(note.is_file() && att[0].is_file(), "Inbox changed");
    assert_eq!(filed_to(&inbox, 502), None);
}

#[test]
fn file_away_missing_dest_dir_is_err() {
    let (inbox, root) = scratch("no-dir");
    let m = msg(503);
    let note = write_note(&inbox, &m).unwrap();
    let missing = root.join("Fidelity/Want to Make");
    assert!(file_away(&inbox, &m, &missing, NAME, FX, LEAD).is_err());
    assert!(!missing.exists(), "file_away created a folder");
    assert!(note.is_file());
    assert_eq!(filed_to(&inbox, 503), None);
}

#[test]
fn filed_to_none_without_ledger_and_skips_junk() {
    let (inbox, _root) = scratch("ledger");
    assert_eq!(filed_to(&inbox, 504), None);
    fs::write(inbox.join(".filed"), "junk line\n\n504 /x/Citadel/Ideas/a.md\n505\n").unwrap();
    assert_eq!(filed_to(&inbox, 504), Some("/x/Citadel/Ideas/a.md".to_string()));
    assert_eq!(filed_to(&inbox, 505), None);
    assert_eq!(filed_to(&inbox, 50), None);
}

/// A planted symlink at the temp name must never be followed: the write stays out of `Private`.
#[test]
fn file_away_tmp_symlink_never_writes_through_into_private() {
    let (inbox, root) = scratch("tmp-symlink");
    let m = msg(506);
    write_note(&inbox, &m).unwrap();
    let private = root.join("Private");
    fs::create_dir_all(&private).unwrap();
    fs::write(private.join("victim.md"), "PRIVATE ORIGINAL\n").unwrap();
    let dest_dir = root.join("Citadel/Ideas");
    let tmp = dest_dir.join(format!(".{NAME}.tmp"));
    std::os::unix::fs::symlink(private.join("victim.md"), &tmp).unwrap();
    let r = file_away(&inbox, &m, &dest_dir, NAME, FX, LEAD);
    assert_eq!(
        fs::read_to_string(private.join("victim.md")).unwrap(),
        "PRIVATE ORIGINAL\n",
        "write went through the temp symlink into Private (file_away -> {r:?})"
    );
    if r.is_ok() {
        let meta = fs::symlink_metadata(dest_dir.join(NAME)).unwrap();
        assert!(meta.is_file() && !meta.file_type().is_symlink(), "filed note is not a regular file");
    }
}

/// The same for the `.filed` ledger temp file in Inbox: a planted link is never followed.
#[test]
fn file_away_ledger_tmp_symlink_never_writes_through() {
    let (inbox, root) = scratch("ledger-symlink");
    let m = msg(507);
    write_note(&inbox, &m).unwrap();
    let private = root.join("Private");
    fs::create_dir_all(&private).unwrap();
    fs::write(private.join("victim.md"), "PRIVATE ORIGINAL\n").unwrap();
    std::os::unix::fs::symlink(private.join("victim.md"), inbox.join(".filed.tmp")).unwrap();
    let dest_dir = root.join("Citadel/Ideas");
    let _ = file_away(&inbox, &m, &dest_dir, NAME, FX, LEAD);
    assert_eq!(
        fs::read_to_string(private.join("victim.md")).unwrap(),
        "PRIVATE ORIGINAL\n",
        "ledger write went through the temp symlink into Private"
    );
}

// ---- Shopping List: appending to a running note, and the bounded lookup bundle (#40). Temp dirs only. ----

const RUNNING_BODY: &str = "# Shopping List\n\n## Costco\n- [ ] Eggs *(Discord 2026-10-01)*\n\n**Hub:** [[Home]]\n";
const RUNNING_REL: &str = "State Street/Cash Flow/Spending/Shopping List.md";

fn text_msg(id: u64, content: &str) -> Incoming {
    Incoming {
        msg_id: id,
        author_id: 1,
        received: "2026-10-13T14:03:22Z".into(),
        content: content.into(),
        attachments: vec![],
        ocr: vec![],
    }
}

/// Plants the running note with `body` under the scratch root and returns its path.
fn plant_running(root: &Path, body: &str) -> PathBuf {
    let p = root.join(RUNNING_REL);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, body).unwrap();
    p
}

#[test]
fn append_running_inserts_block_before_hub_and_records_filed() {
    let (inbox, root) = scratch("append");
    let note = plant_running(&root, RUNNING_BODY);
    let msg = text_msg(400, "- eggs\nmilk");
    let copy = write_note(&inbox, &msg).unwrap();
    append_running(&inbox, &msg, &note, "2026-10-13").unwrap();

    let after = fs::read_to_string(&note).unwrap();
    let hub = RUNNING_BODY.find("**Hub:**").unwrap();
    let added = "- [ ] eggs *(Discord 2026-10-13)*\n- [ ] milk *(Discord 2026-10-13)*\n";
    assert_eq!(after, format!("{}{}\n{}", &RUNNING_BODY[..hub], added, &RUNNING_BODY[hub..]));

    let filed = filed_to(&inbox, 400).expect("ledger line");
    assert!(filed.ends_with("Shopping List.md"), "{filed}");
    assert!(!copy.exists(), "Inbox copy left behind");
}

#[test]
fn append_running_refuses_missing_note_symlink_and_attachments() {
    // Missing: refused, and nothing is created.
    let (inbox, root) = scratch("append-missing");
    let missing = root.join(RUNNING_REL);
    fs::create_dir_all(missing.parent().unwrap()).unwrap();
    let err = append_running(&inbox, &text_msg(401, "eggs"), &missing, "2026-10-13").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);
    assert!(!missing.exists(), "created a missing running note");

    // Symlink: refused, and the link target is untouched.
    let (inbox, root) = scratch("append-link");
    let link = root.join(RUNNING_REL);
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    fs::create_dir_all(root.join("Private")).unwrap();
    fs::write(root.join("Private/note.md"), "private\n").unwrap();
    std::os::unix::fs::symlink(root.join("Private/note.md"), &link).unwrap();
    assert!(append_running(&inbox, &text_msg(401, "eggs"), &link, "2026-10-13").is_err());
    assert_eq!(fs::read_to_string(root.join("Private/note.md")).unwrap(), "private\n");

    // Attachments: refused, so the file is kept by a note instead.
    let (inbox, root) = scratch("append-attach");
    let note = plant_running(&root, RUNNING_BODY);
    let mut msg = text_msg(402, "eggs");
    msg.attachments.push(("receipt.txt".into(), Some(b"receipt".to_vec())));
    let err = append_running(&inbox, &msg, &note, "2026-10-13").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    assert_eq!(fs::read_to_string(&note).unwrap(), RUNNING_BODY);
}

#[test]
fn append_running_masks_card_numbers_and_splits_lines() {
    let (inbox, root) = scratch("append-mask");
    let note = plant_running(&root, RUNNING_BODY);
    let msg = text_msg(403, "4111 1111 1111 1111\ntape");
    write_note(&inbox, &msg).unwrap();
    append_running(&inbox, &msg, &note, "2026-10-13").unwrap();
    let after = fs::read_to_string(&note).unwrap();
    assert!(!after.contains("4111 1111 1111 1111"), "{after}");
    assert!(after.contains("- [ ] tape *(Discord 2026-10-13)*"), "{after}");
}

#[test]
fn lookup_notes_reads_only_targets_inbox_and_running() {
    use discord_inbox::vaults::TARGETS;
    let (inbox, root) = scratch("lookup");
    let mut ledger = String::new();
    let mut id = 10u64;
    // One IN- note in every target folder, each listed in the ledger.
    for (i, t) in TARGETS.iter().enumerate() {
        let d = root.join(t.folder);
        fs::create_dir_all(&d).unwrap();
        let p = d.join("x.md");
        fs::write(&p, format!("IN-{i}\n")).unwrap();
        ledger.push_str(&format!("{id} {}\n", p.display()));
        id += 1;
    }
    // OUT- notes: under Private, deeper under Private, outside the root, and a symlink into Private.
    fs::create_dir_all(root.join("Private/deep")).unwrap();
    fs::write(root.join("Private/note.md"), "OUT-private private\n").unwrap();
    fs::write(root.join("Private/deep/x.md"), "OUT-deep\n").unwrap();
    let outside = root.parent().unwrap().join(format!("di-filing-{}-outside", std::process::id()));
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("o.md"), "OUT-outside\n").unwrap();
    std::os::unix::fs::symlink(root.join("Private/note.md"), root.join("State Street/l.md")).unwrap();
    for p in [
        root.join("Private/note.md"),
        root.join("Private/deep/x.md"),
        outside.join("o.md"),
        root.join("State Street/l.md"),
    ] {
        ledger.push_str(&format!("{id} {}\n", p.display()));
        id += 1;
    }
    fs::write(inbox.join(".filed"), ledger).unwrap();
    // An Inbox note with a card number: included, masked.
    fs::write(inbox.join("2026-10-13 Costco.md"), "IN-inbox card 4111 1111 1111 1111\n").unwrap();

    let allowed: Vec<PathBuf> = TARGETS.iter().map(|t| root.join(t.folder)).collect();
    let got = lookup_notes(&inbox, &allowed, &[], 1_000_000);
    let all: String = got.iter().map(|(l, t)| format!("{l}\n{t}\n")).collect();
    for i in 0..TARGETS.len() {
        assert!(all.contains(&format!("IN-{i}")), "missing IN-{i}: {all}");
    }
    assert!(all.contains("IN-inbox"), "{all}");
    assert!(!all.contains("OUT-"), "{all}");
    assert!(!all.to_lowercase().contains("private"), "{all}");
    assert!(!all.contains("l.md"), "{all}");
    assert!(!all.contains("4111 1111 1111 1111"), "{all}");
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn lookup_notes_respects_cap_and_order() {
    let (inbox, root) = scratch("lookup-cap");
    fs::write(inbox.join("2026-10-12 a.md"), "a".repeat(500)).unwrap();
    fs::write(inbox.join("2026-10-13 b.md"), "b".repeat(500)).unwrap();
    let run = plant_running(&root, "- [ ] SHOP\n");

    let capped = lookup_notes(&inbox, &[], &[run.clone()], 100);
    let total: usize = capped.iter().map(|(l, t)| l.chars().count() + t.chars().count()).sum();
    assert!(total <= 100, "{total}");

    let all = lookup_notes(&inbox, &[], &[run], 1_000_000);
    assert!(all.len() >= 3, "{all:?}");
    assert!(all[0].1.contains("SHOP"), "running note not first: {all:?}");
    assert!(all[1].0.ends_with("2026-10-13 b.md"), "Inbox not newest first: {all:?}");
    assert!(all[2].0.ends_with("2026-10-12 a.md"), "{all:?}");
}

#[test]
fn append_running_keeps_leading_emphasis() {
    let (inbox, root) = scratch("append-emphasis");
    let note = plant_running(&root, RUNNING_BODY);
    let msg = text_msg(405, "*eggs* and milk");
    write_note(&inbox, &msg).unwrap();
    append_running(&inbox, &msg, &note, "2026-10-13").unwrap();
    let after = fs::read_to_string(&note).unwrap();
    assert!(after.contains("- [ ] *eggs* and milk *(Discord 2026-10-13)*"), "{after}");
}

#[test]
fn append_running_strips_only_bullet_marker() {
    let (inbox, root) = scratch("append-bullet");
    let note = plant_running(&root, RUNNING_BODY);
    let msg = text_msg(406, "- [ ] eggs");
    write_note(&inbox, &msg).unwrap();
    append_running(&inbox, &msg, &note, "2026-10-13").unwrap();
    let after = fs::read_to_string(&note).unwrap();
    assert!(after.contains("- [ ] eggs *(Discord 2026-10-13)*"), "{after}");
    assert!(!after.contains("[ ] [ ]"), "{after}");
}

#[test]
fn lookup_notes_keeps_tail_of_long_running_list() {
    let (inbox, root) = scratch("lookup-tail");
    let mut body = String::from("## Costco\n");
    for i in 0..300 {
        body.push_str(&format!("- [ ] item {i:03}\n"));
    }
    let run = plant_running(&root, &body);
    let notes = lookup_notes(&inbox, &[], &[run], 12_000);
    assert_eq!(notes.len(), 1, "running note missing from lookup");
    assert!(notes[0].1.contains("item 299"), "running list cut before its last item");
}

#[test]
fn lookup_notes_caps_running_note_at_its_tail() {
    let (inbox, root) = scratch("lookup-big");
    let body = format!("{}\n- [ ] last one\n", "x".repeat(10_000));
    let run = plant_running(&root, &body);
    let notes = lookup_notes(&inbox, &[], &[run], 100_000);
    assert_eq!(notes.len(), 1, "running note missing from lookup");
    assert!(notes[0].1.contains("last one"), "newest item cut");
    assert!(notes[0].1.chars().count() <= 6_000, "{}", notes[0].1.chars().count());
}
