//! Breaker tests for PR #113 (issue #112): cat touches still sent as Stormy replies after the persona change.
use discord_inbox::ingest::BLANK_REPLY;
use discord_inbox::pantry::{self, Report, Row};
use discord_inbox::vaults::PANTRY;
use std::fs;

const PAW: char = '\u{1F43E}';

/// Reply sent for a DM with nothing to read (ingest.rs BLANK_REPLY).
#[test]
fn breaker_blank_reply_has_no_paw() {
    assert!(!BLANK_REPLY.contains(PAW), "BLANK_REPLY is a Stormy reply: {BLANK_REPLY:?}");
}

/// `restock` reply text (pantry.rs Report::text).
#[test]
fn breaker_restock_reply_has_no_paw() {
    let r = Report { source: "Costco".into(), added: vec!["eggs".into()], others: 0, unassigned: vec![], oldest: vec![] };
    let text = r.text();
    assert!(!text.contains(PAW), "{text:?}");
}

/// `out of eggs` reply text (pantry.rs out_of, the "Dropped ... 🐾" line).
#[test]
fn breaker_out_of_reply_has_no_paw() {
    let dir = std::env::temp_dir().join(format!("breaker-113-outof-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let note = dir.join(PANTRY);
    fs::create_dir_all(note.parent().unwrap()).unwrap();
    let rows = vec![Row { item: "eggs".into(), qty: "1".into(), store: "Costco".into(), bought: "2026-10-02".into() }];
    fs::write(&note, pantry::render(&rows, "2026-10-15")).unwrap();
    let out = pantry::out_of(&dir, "eggs", "2026-10-15").unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(out.starts_with("Dropped eggs"), "fixture did not drop the row: {out:?}");
    assert!(!out.contains(PAW), "{out:?}");
}

/// Source scan (not behavioural): the "foggy" and "couldn't get to your notes" replies are literals inside
/// ingest.rs handlers, so no public function returns them. Fails on any cat-face or cat emoji literal in reply code.
#[test]
fn breaker_reply_source_has_no_cat_face_emoji() {
    let cats = [
        '\u{1F63A}', '\u{1F638}', '\u{1F639}', '\u{1F63B}', '\u{1F63C}', '\u{1F63D}', '\u{1F63E}', '\u{1F63F}', '\u{1F640}',
        '\u{1F431}', '\u{1F408}',
    ];
    let mut hits = Vec::new();
    for file in ["src/ingest.rs", "src/pantry.rs", "src/commands.rs"] {
        let src = fs::read_to_string(format!("{}/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        for (n, line) in src.lines().enumerate() {
            if line.contains(&cats[..]) {
                hits.push(format!("{file}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(hits.is_empty(), "cat-face emoji still in reply code:\n{}", hits.join("\n"));
}
