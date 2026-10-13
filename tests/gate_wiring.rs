//! Pins the gate wiring in `src/main.rs`, which the lib tests cannot reach.
//! A source check: crude on purpose, and it fails if the wiring is edited away.
use std::fs;

fn main_rs() -> String {
    fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs")).unwrap()
}

#[test]
fn ingest_checks_the_allowlist_before_anything_else() {
    let src = main_rs();
    let at = src.find("async fn ingest(").expect("ingest exists");
    let body = &src[at..];
    let open = body.find('{').unwrap();
    let first = body[open + 1..]
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("//"))
        .unwrap();
    assert!(
        first.starts_with("if !h.allow.contains(msg.author.id.get())"),
        "first statement of ingest is {first:?}"
    );
}

#[test]
fn main_rs_only_writes_through_accept() {
    let src = main_rs();
    for banned in ["write_note", "write_last_id", "admit_and_write"] {
        assert!(!src.contains(banned), "main.rs must not call {banned}");
    }
    assert_eq!(src.matches("dump::accept(").count(), 1);
}

#[test]
fn react_happens_once_and_only_after_accept() {
    let src = main_rs();
    assert_eq!(src.matches(".react(").count(), 1, "one react call");
    let accept = src.find("dump::accept(").expect("accept call");
    let react = src.find(".react(").unwrap();
    assert!(accept < react, "react must come after accept");
}
