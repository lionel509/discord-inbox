//! The one glue check on `src/main.rs`. The logic is tested in the lib, through `tests/ingest.rs`.
//! This is a source check, crude on purpose: it fails if `main.rs` grows its own write, gate or cursor.
use std::fs;

fn main_rs() -> String {
    fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs")).unwrap()
}

/// The text from `start` up to the next `async fn ` (or the end of the file).
fn body_after<'a>(src: &'a str, start: &str) -> &'a str {
    let at = src.find(start).unwrap_or_else(|| panic!("{start} missing"));
    let rest = &src[at + start.len()..];
    match rest.find("async fn ") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

#[test]
fn main_rs_is_glue_over_the_lib() {
    let src = main_rs();
    assert!(
        body_after(&src, "async fn message(").contains("ingest_with("),
        "message must call ingest_with (the Filer-taking lib entry, plan step 6)"
    );
    assert!(
        body_after(&src, "async fn ready(").contains("catch_up_with("),
        "ready must call catch_up_with (the Filer-taking lib entry, plan step 6)"
    );
    assert!(
        !body_after(&src, "async fn message(").contains("Filer::off"),
        "message must use the live Filer built in main, never Filer::off"
    );
    for banned in [
        "write_note",
        "accept(",
        "write_cursor",
        "read_cursor",
        "add_retry",
        "remove_retry",
        "allow.contains",
    ] {
        assert!(!src.contains(banned), "main.rs must not contain {banned}");
    }
    // The serenity adapter lives in src/live.rs (planned verbatim move); main.rs must hold none of it.
    let live = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/live.rs")).unwrap();
    assert_eq!(src.matches(".react(").count(), 0, "main.rs must not call .react(");
    assert_eq!(src.matches(".download(").count(), 0, "main.rs must not call .download(");
    assert_eq!(
        src.matches(".react(").count() + live.matches(".react(").count(),
        1,
        "one react path across main.rs + live.rs, in the adapter"
    );
    assert_eq!(
        src.matches(".download(").count() + live.matches(".download(").count(),
        1,
        "one download path across main.rs + live.rs, in the adapter"
    );
}
