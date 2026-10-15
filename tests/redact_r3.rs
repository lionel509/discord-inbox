// Breaker round 3 on 92f7f34: redaction gaps (#34 round 3). Stubs only, made-up values. Each test asserts the safe output.
// Ignored tests are follow-up #39 items and stay out of this round's blocking set.
use discord_inbox::chat;
use discord_inbox::dump::{write_note, Incoming};
use discord_inbox::ingest::combine;
use discord_inbox::redact::sensitive;
use std::fs;

fn msg(content: &str, attachments: Vec<(String, Option<Vec<u8>>)>) -> Incoming {
    Incoming { msg_id: 9001, author_id: 42, received: "2026-10-14T12:00:00Z".into(), content: content.into(), attachments, ocr: vec![] }
}

fn note_for(name: &str, content: &str, attachments: Vec<(String, Option<Vec<u8>>)>) -> String {
    let root = std::env::temp_dir().join(format!("di-breaker3-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let inbox = root.join("Inbox");
    let path = write_note(&inbox, &msg(content, attachments)).expect("write_note");
    fs::read_to_string(path).unwrap()
}

// B1: a Luhn-valid card after "order #" with spaces in groups of 4 (the common format) comes back whole.
#[test]
fn b1_card_after_order_label_grouped_by_four() {
    assert_eq!(sensitive("order #4111 1111 1111 1111"), "order #**** **** **** 1111");
}

// B1 at each application point: note on disk, model input, Discord reply.
#[test]
fn b1_note_on_disk_keeps_card_after_ref() {
    let note = note_for("ref", "Ref: 4111 1111 1111 1111", vec![]);
    assert!(!note.contains("4111 1111"), "note body leaks card: {note}");
}

#[test]
fn b1_model_input_keeps_card_after_ref() {
    let body = chat::request_body("sys", "Ref: 4111 1111 1111 1111").to_string();
    assert!(!body.contains("4111 1111"), "model request leaks card: {body}");
}

#[test]
fn b1_discord_reply_keeps_card_after_ref() {
    let reply = combine(None, "Ref: 4111 1111 1111 1111", None);
    assert!(!reply.contains("4111 1111"), "reply leaks card: {reply}");
}

// B2: a card whose line is joined to a preceding order number by a newline (gaps include newlines).
#[test]
fn b2_card_on_next_line_after_order_number() {
    let out = sensitive("order #113-1234567-1234567\n4111 1111 1111 1111");
    assert!(!out.contains("4111 1111"), "card on the line after an order number leaks: {out:?}");
}

// Item 2 of the round 3 review: a card on the same line as an order number joins into one run over 19 digits.
#[test]
fn card_after_order_number_same_line() {
    let out = sensitive("Order #113-1234567-1234567 4111 1111 1111 1111");
    assert!(!out.contains("4111 1111"), "card after an order number on the same line leaks: {out:?}");
}

// B3: a Luhn-invalid card (an OCR misread of one digit) after a label is kept whole.
#[ignore = "follow-up #39"]
#[test]
fn b3_ocr_misread_card_after_label() {
    let out = sensitive("Ref: 4111 1111 1111 1112");
    assert!(!out.contains("4111 1111"), "misread card after ref leaks: {out:?}");
}

// B4: SSN without dashes, and with spaces, after the SSN label.
#[test]
fn b4_ssn_without_dashes_after_label() {
    assert_eq!(sensitive("SSN 123456789"), "SSN ***-**-6789");
}

#[test]
fn b4_ssn_with_spaces_after_label() {
    assert_eq!(sensitive("SSN: 123 45 6789"), "SSN: ***-**-6789");
}

// B5: token families missing from the prefix list.
#[test]
fn b5_stripe_github_oauth_shopify_tokens() {
    for s in [
        "sk_live_FAKEabcdefghijkl",
        "rk_live_FAKEabcdefghijkl",
        "pk_live_FAKEabcdefghijkl",
        "gho_FAKEabcdefghijkl",
        "sk_test_FAKEabcdefghijkl",
        "shpat_FAKEabcdefghijkl",
        "shppa_FAKEabcdefghijkl",
    ] {
        let out = sensitive(s);
        assert!(!out.contains("abcdef"), "token leaks: {s} -> {out}");
    }
}

// B6: labelled secrets with no `=`, and labels not in the list.
#[test]
fn b6_labelled_secrets_without_equals() {
    for s in ["api_key: abcd1234efgh5678ijkl", "password is hunter2", "token: abcd1234efgh5678"] {
        let out = sensitive(s);
        assert!(!out.contains("hunter2") && !out.contains("abcd1234"), "secret leaks: {s} -> {out}");
    }
}

// B6, bare form: "password hunter2" would also mask "password reset", so it waits for #39.
#[ignore = "follow-up #39"]
#[test]
fn b6_bare_password_word() {
    let out = sensitive("password hunter2");
    assert!(!out.contains("hunter2"), "bare password leaks: {out}");
}

// B7: Basic auth and credentials inside a URL.
#[test]
fn b7_basic_auth_and_url_credentials() {
    for s in ["Authorization: Basic dXNlcjpodW50ZXIy", "https://lionel:hunter2@example.com/x"] {
        let out = sensitive(s);
        assert!(!out.contains("dXNlcjpodW50ZXIy") && !out.contains("hunter2"), "credential leaks: {s} -> {out}");
    }
}

// B8: a Bearer token under the 20-character minimum.
#[test]
fn b8_short_bearer_token() {
    assert_eq!(sensitive("Authorization: Bearer abc123def456ghi7"), "Authorization: Bearer [redacted]");
}

// B9: a card written with its attachment's filename reaches the note's frontmatter unmasked.
#[ignore = "follow-up #39"]
#[test]
fn b9_attachment_filename_with_card_number() {
    let note = note_for("fname", "see attached", vec![("statement_4111111111111111.pdf".into(), None)]);
    assert!(!note.contains("4111111111111111"), "attachment name leaks card: {note}");
}

// O1 to O5: over-redaction of content Lionel needs.
#[test]
fn o1_amazon_order_id_survives() {
    assert_eq!(sensitive("Order ID: 113-1234567-1234567"), "Order ID: 113-1234567-1234567");
}

#[ignore = "follow-up #39"]
#[test]
fn o2_isbn13_survives() {
    assert_eq!(sensitive("ISBN 978-0-13-468599-1"), "ISBN 978-0-13-468599-1");
}

#[ignore = "follow-up #39"]
#[test]
fn o3_usps_tracking_survives() {
    assert_eq!(sensitive("Tracking 9400 1000 0000 0000 0000 00"), "Tracking 9400 1000 0000 0000 0000 00");
}

#[test]
fn o4_international_phone_survives() {
    assert_eq!(sensitive("Phone +86 138 0013 8000"), "Phone +86 138 0013 8000");
}

#[test]
fn o5_receipt_number_survives() {
    assert_eq!(sensitive("Receipt No. 20261014123456"), "Receipt No. 20261014123456");
}
