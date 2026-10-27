//! Rework round 1 on PR #114, area U3 (check-in marker writes). Made-up fixtures only.
//! Pins finding 2: with the marker folder unwritable for a whole pass, no DM may go out (a DM whose marker cannot be
//! written would repeat on every pass). Once the folder is writable again, each id gets its check-in exactly once.
use discord_inbox::checkin;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("breaker-114-rw1-u3-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

#[tokio::test]
async fn whole_pass_read_only_sends_nothing_then_each_id_once() {
    let dir = scratch("readonly-pass");
    fs::create_dir_all(&dir).unwrap();
    let ids = [11u64, 22];
    let day = "2026-10-26";
    let counts: RefCell<HashMap<u64, u32>> = RefCell::new(HashMap::new());

    // The folder is unwritable for the whole pass, so no marker can be staged and no DM may be sent.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let first = checkin::send_all(&dir, day, &ids, |uid, _body| {
        *counts.borrow_mut().entry(uid).or_insert(0) += 1;
        async { Ok::<(), String>(()) }
    })
    .await;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(first.is_err(), "an unwritable marker folder must be reported, got {first:?}");
    assert!(counts.borrow().is_empty(), "no DM may go out while its marker cannot be written, got {:?}", counts.borrow());

    // Folder writable again: one more pass sends exactly once per id.
    let second = checkin::send_all(&dir, day, &ids, |uid, _body| {
        *counts.borrow_mut().entry(uid).or_insert(0) += 1;
        async { Ok::<(), String>(()) }
    })
    .await;
    assert!(second.is_ok(), "second pass should succeed, got {second:?}");

    let counts = counts.into_inner();
    for uid in ids {
        assert_eq!(counts.get(&uid).copied(), Some(1), "id {uid} must get the check-in exactly once (counts {counts:?})");
    }
}
