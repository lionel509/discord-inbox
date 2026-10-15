//! Deterministic redaction (#34 round 2): card numbers, SSNs, labelled bank and routing numbers and secrets
//! are masked on the note written to disk, in every model input, and in the Discord reply. Made-up data only.
use discord_inbox::chat;
use discord_inbox::dump::{file_away, write_note, Incoming};
use discord_inbox::ingest::combine;
use discord_inbox::insight;
use std::fs;
use std::path::PathBuf;

/// A temp Inbox and vault root with a `Citadel/Ideas` folder.
fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("di-redact-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox).unwrap();
    fs::create_dir_all(root.join("Citadel/Ideas")).unwrap();
    (inbox, root)
}

/// A reply with no filing and no verdict is the chat text alone, with no status label (#45).
fn reply(text: &str) -> String {
    combine(None, text, None)
}

fn kept(expected: &str) -> String {
    expected.to_string()
}

// ---- Categories, through the reply path ----

#[test]
fn category_ssn_keeps_last_four() {
    assert_eq!(reply("SSN 123-45-6789 on file"), kept("SSN ***-**-6789 on file"));
}

#[test]
fn category_labelled_account_and_routing_keep_last_four() {
    assert_eq!(reply("account 123456789012"), kept("account ********9012"));
    assert_eq!(reply("acct: 9876543210"), kept("acct: ******3210"));
    assert_eq!(reply("routing 021000021"), kept("routing *****0021"));
    assert_eq!(reply("ABA # 021000021"), kept("ABA # *****0021"));
}

#[test]
fn category_api_keys_and_tokens_are_redacted() {
    assert_eq!(reply("key sk-proj-abcdefghijklmnopqrstuv12 ok"), kept("key [redacted] ok"));
    assert_eq!(
        reply("token ghp_abcdefghijklmnopqrstuvwxyz0123456789 ok"),
        kept("token [redacted] ok")
    );
    assert_eq!(
        reply("github_pat_11ABCDEFG0123456789_abcdefghijklmnop end"),
        kept("[redacted] end")
    );
    assert_eq!(reply("slack xoxb-1234567890-abcdefghij end"), kept("slack [redacted] end"));
    assert_eq!(reply("aws AKIAIOSFODNN7EXAMPLE end"), kept("aws [redacted] end"));
}

#[test]
fn category_bearer_token_is_redacted() {
    assert_eq!(
        reply("Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123 done"),
        kept("Authorization: Bearer [redacted] done")
    );
}

#[test]
fn category_password_value_is_redacted_label_kept() {
    assert_eq!(reply("password=hunter22 next"), kept("password=[redacted] next"));
    assert_eq!(reply("pwd: s3cr3t next"), kept("pwd: [redacted] next"));
}

// ---- Application points ----

#[test]
fn app_disk_inbox_note_masks_body_and_ocr() {
    let (inbox, _root) = scratch("disk");
    let msg = Incoming {
        msg_id: 9001,
        author_id: 1,
        received: "2026-10-14T12:00:00Z".into(),
        content: "paid card 4111 1111 1111 1111, SSN 123-45-6789".into(),
        attachments: vec![("receipt.png".into(), Some(b"img".to_vec()))],
        ocr: vec![(0, "VISA 4111 1111 1111 1111 TOTAL $9.70".into())],
    };
    let path = write_note(&inbox, &msg).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("4111 1111"), "{text}");
    assert!(!text.contains("123-45-6789"), "{text}");
    assert!(text.contains("**** **** **** 1111"), "{text}");
    assert!(text.contains("***-**-6789"), "{text}");
    assert!(text.contains("TOTAL $9.70"), "{text}");
}

#[test]
fn app_disk_filed_note_masks_body_and_ocr() {
    let (inbox, root) = scratch("filed");
    let msg = Incoming {
        msg_id: 9002,
        author_id: 1,
        received: "2026-10-14T12:05:00Z".into(),
        content: "sketch, key sk-proj-abcdefghijklmnopqrstuv12".into(),
        attachments: vec![("receipt.png".into(), Some(b"img".to_vec()))],
        ocr: vec![(0, "password=hunter22 TOTAL $9.70".into())],
    };
    write_note(&inbox, &msg).unwrap();
    let dest_dir = root.join("Citadel/Ideas");
    let dest = file_away(&inbox, &msg, &dest_dir, "2026-10-14 Sketch.md", "tags:\n  - citadel\n", "Hub: [[Ideas]]\n\n")
        .unwrap();
    let text = fs::read_to_string(&dest).unwrap();
    assert!(!text.contains("sk-proj"), "{text}");
    assert!(!text.contains("hunter22"), "{text}");
    assert!(text.contains("password=[redacted]"), "{text}");
    assert!(text.contains("TOTAL $9.70"), "{text}");
}

#[test]
fn app_model_mimo_request_body_masks_card_and_ssn() {
    let body = chat::request_body("system", "card 4111 1111 1111 1111 ssn 123-45-6789").to_string();
    assert!(!body.contains("4111 1111"), "{body}");
    assert!(!body.contains("123-45-6789"), "{body}");
    assert!(body.contains("***-**-6789"), "{body}");
}

#[test]
fn app_model_haiku_prompt_masks_secret() {
    let p = insight::prompt("VISA 4111 1111 1111 1111\npassword=hunter22");
    assert!(!p.contains("4111 1111"), "{p}");
    assert!(!p.contains("hunter22"), "{p}");
    assert!(p.contains("**** **** **** 1111"), "{p}");
}

#[test]
fn app_model_scaledown_request_masks_ssn_and_key() {
    let body = insight::scaledown_request("SSN 123-45-6789 key sk-proj-abcdefghijklmnopqrstuv12");
    assert!(!body.contains("123-45-6789"), "{body}");
    assert!(!body.contains("sk-proj"), "{body}");
}

#[test]
fn app_reply_masks_ssn_key_and_password() {
    let out = combine(Some("State Street/x.md"), "SSN 123-45-6789 password=hunter22", Some("Receipt: A · $1.00"));
    assert!(!out.contains("123-45-6789"), "{out}");
    assert!(!out.contains("hunter22"), "{out}");
    assert!(out.contains("***-**-6789"), "{out}");
}

// ---- Guards: these must come back unchanged ----

#[test]
fn guard_hua_lian_receipt_is_unchanged() {
    let receipt = "華聯超市\n牛奶 2瓶 $15.48\n麵包 $16.00\n稅 $0.52\n10/13/2026 16:22:00\n\
                   (718) 321-3388\nStation 05 313524394562\nCashier # 8189\nTOTAL $16.00";
    assert_eq!(reply(receipt), kept(receipt));
    let body = insight::scaledown_request(receipt);
    assert!(body.contains("313524394562"), "{body}");
    assert!(body.contains("10/13/2026 16:22:00"), "{body}");
}

#[test]
fn guard_order_number_is_unchanged() {
    let text = "Order #112-7654321-0987654 shipped Tuesday";
    assert_eq!(reply(text), kept(text));
}
