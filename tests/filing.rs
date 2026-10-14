//! Moving a note and its attachments into a vault folder, and the `.filed` ledger. Temp dirs only.
use discord_inbox::dump::{file_away, filed_to, write_note, Incoming};
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
