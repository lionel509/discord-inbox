// Breaker round 4 on 4a5f649: "password is: X" leaks X (#34 round 4). Stubs only, made-up values.
use discord_inbox::redact::sensitive;

// L4: the optional colon after "is" must be skipped, so the value after it is masked.
#[test]
fn r4_password_is_with_colon() {
    let out = sensitive("password is: hunter2");
    assert!(!out.contains("hunter2"), "leaked: {out}");
    assert_eq!(out, "password is: [redacted]");

    let out = sensitive("the password is: hunter2");
    assert!(!out.contains("hunter2"), "leaked: {out}");
    assert_eq!(out, "the password is: [redacted]");

    let out = sensitive("password is : hunter2");
    assert!(!out.contains("hunter2"), "leaked: {out}");

    // Without the colon, the value after "is" is still masked.
    assert_eq!(sensitive("my password is hunter2"), "my password is [redacted]");

    // Guard: only the word after "is" is masked, the rest of the sentence stays.
    assert_eq!(sensitive("the password is wrong"), "the password is [redacted]");
}
