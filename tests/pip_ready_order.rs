//! Source-order check on the Pip bin's `ready`: catch-up must run before the check-in loop starts.
//! Crude on purpose, like tests/gate_wiring.rs: it reads `src/bin/pip.rs`.
use std::fs;

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
fn ready_catches_up_before_the_checkin_loop_starts() {
    let src = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bin/pip.rs")).unwrap();
    let ready = body_after(&src, "async fn ready(");
    let take = ready.find(".take()").expect("ready must abort the old check-in loop with .take()");
    let catch_up = ready.find("catch_up_each(").expect("ready must call catch_up_each");
    let install = ready.find("install_checkin_loop(").expect("ready must install the check-in loop");
    assert!(take < catch_up, "the old loop must be aborted before catch-up");
    assert!(catch_up < install, "catch-up must run before the check-in loop starts (a 21:00 tick must not beat the replayed DMs)");
}
