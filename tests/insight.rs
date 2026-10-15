//! The receipt reply: model line parsing, card masking, budget note reading, reply layout. Made-up data only.
use discord_inbox::insight::{self, Insight};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

const BUDGET_OCT: &str = "budget: $800\n- 2026-10-02 Groceries $45.10\n| 2026-10-05 | Lunch | $12.90 |\n- 2026-10-07 Notes (no amount)\n- Notes $999\n$1,000 in prose\n";
const BUDGET_NOV: &str = "budget: $800\n- 2026-11-02 Big $812.00\n";

/// `DISCORD_INBOX_BUDGET` is process-global, so it is set once for this binary before any test reads it.
fn dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("di-insight-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("2026-10.md"), BUDGET_OCT).unwrap();
        fs::write(dir.join("2026-11.md"), BUDGET_NOV).unwrap();
        std::env::set_var("DISCORD_INBOX_BUDGET", dir.join("{month}.md"));
        dir
    })
}

fn receipt(merchant: &str, total: &str, category: &str) -> Insight {
    Insight {
        receipt: true,
        merchant: merchant.to_string(),
        total: total.to_string(),
        category: category.to_string(),
    }
}

#[test]
fn parse_reads_four_lines() {
    let got = insight::parse("receipt: yes\nmerchant: Trader Joe's\ntotal: 12.34\ncategory: groceries\n");
    assert_eq!(got, Some(receipt("Trader Joe's", "12.34", "groceries")));

    let no = insight::parse("Receipt: NO\nmerchant: \n").unwrap();
    assert!(!no.receipt);
    assert_eq!(
        (no.merchant.as_str(), no.total.as_str(), no.category.as_str()),
        ("unknown", "unknown", "unknown")
    );

    assert_eq!(insight::parse("hello"), None);

    let cat = insight::parse("category: Dining\nreceipt: yes").unwrap();
    assert_eq!(cat.category, "dining");
}

#[test]
fn mask_cards_keeps_last_four() {
    assert_eq!(insight::mask_cards("VISA 4111 1111 1111 1111"), "VISA **** **** **** 1111");
    assert_eq!(insight::mask_cards("4111-1111-1111-1111"), "****-****-****-1111");
    assert_eq!(insight::mask_cards("4111111111111111"), "************1111");
    assert_eq!(insight::mask_cards("123456789012"), "********9012");
    for same in ["VISA 1234", "631-555-0100", "TOTAL 12.34", "2026-10-13 14:03", "총액 12.34"] {
        assert_eq!(insight::mask_cards(same), same);
    }
}

#[test]
fn mask_cards_invariant_random() {
    // A 64-bit LCG, seed 42, so every run draws the same strings.
    let mut x: u64 = 42;
    let mut draw = |n: u64| -> u64 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) % n
    };
    for _ in 0..300 {
        let mut input = String::new();
        for _ in 0..draw(10) {
            match draw(4) {
                0 => input.push_str("TOTAL"),
                1 => input.push_str("총"),
                2 => {
                    let len = 1 + draw(20);
                    for _ in 0..len {
                        input.push(char::from(b'0' + draw(10) as u8));
                    }
                }
                _ => input.push_str(if draw(2) == 0 { " " } else { "-" }),
            }
        }
        let out = insight::mask_cards(&input);
        // Only digits change, and only into '*': everything else is copied as is.
        let strip = |s: &str| -> String { s.chars().filter(|c| !c.is_ascii_digit() && *c != '*').collect() };
        assert_eq!(strip(&input), strip(&out), "{input:?} -> {out:?}");
        let stars = out.chars().filter(|c| *c == '*').count();
        let digits_in = input.chars().filter(|c| c.is_ascii_digit()).count();
        let digits_out = out.chars().filter(|c| c.is_ascii_digit()).count();
        assert_eq!(digits_in, digits_out + stars, "{input:?} -> {out:?}");
    }
}

#[test]
fn budget_fixture_parses() {
    dir();
    assert_eq!(insight::parse_budget(BUDGET_OCT), Some((800.0, 58.0)));
    assert_eq!(insight::parse_budget("groceries $10\n- 2026-10-02 x $5\n"), None);

    let line = insight::budget_line("2026-10-13T14:03:22Z");
    assert_eq!(line.as_deref(), Some("Budget left before this: $742.00 of $800.00 (2026-10)"));

    let over = insight::budget_line("2026-11-01T00:00:00Z");
    assert_eq!(over.as_deref(), Some("Budget left before this: -$12.00 of $800.00 (2026-11)"));

    assert_eq!(insight::budget_line("2025-01-05T00:00:00Z"), None);
    let path = insight::budget_path("2026-10-13T14:03:22Z").unwrap();
    assert_eq!(path, dir().join("2026-10.md"));
}

#[test]
fn ask_runs_command_and_times_out() {
    dir();
    let say = |body: &str| -> Vec<String> { vec!["/bin/sh".into(), "-c".into(), format!("cat >/dev/null; {body}")] };
    let ok = say("printf 'receipt: yes\\nmerchant: Shop\\ntotal: 1.00\\ncategory: other\\n'");
    assert_eq!(
        insight::ask(&ok, "TRADER JOES", Duration::from_secs(5)),
        Some(receipt("Shop", "1.00", "other"))
    );

    let slow = say("sleep 5");
    let started = std::time::Instant::now();
    assert_eq!(insight::ask(&slow, "x", Duration::from_secs(1)), None);
    assert!(started.elapsed() < Duration::from_secs(3));

    assert_eq!(insight::ask(&[], "x", Duration::from_secs(1)), None);
    assert_eq!(insight::ask(&say("exit 1"), "x", Duration::from_secs(5)), None);
}

#[test]
fn reply_text_fits_discord() {
    let r = receipt("Trader Joe's", "12.34", "groceries");
    let long = "x".repeat(6000);
    let text = insight::reply_text(&[("a.png".into(), long, Some(r.clone()))], Some("Budget left before this: $742.00 of $800.00 (2026-10)".into()));
    assert!(text.chars().count() <= 2000, "{}", text.chars().count());
    assert!(text.ends_with('…'));
    assert!(text.starts_with("Receipt: Trader Joe's · $12.34 · groceries"), "{text}");

    let none = insight::reply_text(&[("a.png".into(), "TOTAL 1".into(), None)], None);
    assert_eq!(none, "TOTAL 1");

    let budget = insight::reply_text(&[("a.png".into(), "t".into(), Some(r.clone()))], Some("Budget left before this: $1.00 of $2.00 (2026-10)".into()));
    assert!(budget.contains("Budget left before this"), "{budget}");
    let no_budget = insight::reply_text(&[("a.png".into(), "t".into(), Some(r))], None);
    assert!(!no_budget.contains("Budget"), "{no_budget}");

    let two = insight::reply_text(
        &[
            ("a.png".into(), "one".into(), Some(receipt("S", "1.00", "other"))),
            ("b.png".into(), "two".into(), Some(Insight { receipt: false, merchant: "unknown".into(), total: "unknown".into(), category: "unknown".into() })),
        ],
        None,
    );
    assert!(two.contains("a.png:") && two.contains("b.png:"), "{two}");
    assert!(two.contains("Not a receipt: b.png"), "{two}");

    let unknown = insight::reply_text(&[("a.png".into(), "t".into(), Some(receipt("S", "unknown", "other")))], None);
    assert!(unknown.starts_with("Receipt: S · unknown · other"), "{unknown}");
}

#[test]
fn parse_reads_bold_and_bulleted_labels() {
    let bold = insight::parse("**receipt:** yes\n**merchant:** Trader Joe's\n**total:** 42.10\n**category:** groceries");
    assert_eq!(bold, Some(receipt("Trader Joe's", "42.10", "groceries")));
    let bullet = insight::parse("- receipt: yes\n- merchant: Trader Joe's\n- total: 42.10\n- category: groceries");
    assert_eq!(bullet, Some(receipt("Trader Joe's", "42.10", "groceries")));
}

#[test]
fn mask_cards_bridges_a_line_break() {
    let out = insight::mask_cards("Card 4111 1111\n1111 1111");
    assert!(!out.contains("4111"), "{out:?}");
    assert!(out.ends_with("1111"), "{out:?}");
    assert_eq!(insight::mask_cards("Total $42.10\n2026"), "Total $42.10\n2026");
}

#[test]
fn budget_skips_a_heading_without_money() {
    let note = "# Budget 2026-10\n\nbudget: $800\n- 2026-10-02 Groceries $45.10\n";
    assert_eq!(insight::parse_budget(note), Some((800.0, 45.10)));
}

#[tokio::test]
async fn model_command_keeps_a_quoted_path_and_masks_its_reply() {
    dir();
    // Both checks share one test because DISCORD_INBOX_INSIGHT is process-global.
    std::env::set_var("DISCORD_INBOX_INSIGHT", "\"/Users/test/Discord Inbox/bin/model\" --flag");
    assert_eq!(
        insight::model(),
        insight::Model::Cmd(vec!["/Users/test/Discord Inbox/bin/model".to_string(), "--flag".to_string()])
    );

    // A stub model that echoes a card number in its merchant line, as a photo of a card could.
    let stub = dir().join("insight_stub.sh");
    fs::write(
        &stub,
        "#!/bin/sh\ncat >/dev/null\nprintf 'receipt: yes\\nmerchant: VISA 4111 1111 1111 1111\\ntotal: 1.00\\ncategory: other\\n'\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("DISCORD_INBOX_INSIGHT", stub.to_string_lossy().to_string());

    let msg = discord_inbox::dump::Incoming {
        msg_id: 1,
        author_id: 1,
        received: "2026-10-13T14:03:22Z".into(),
        content: String::new(),
        attachments: vec![("photo.jpg".into(), None)],
        ocr: vec![(0, "TOTAL 4111 1111 1111 1111".into())],
    };
    let reply = insight::compose(&msg).await.expect("a reply");
    assert!(!reply.contains("4111"), "{reply}");
    assert!(reply.contains("Receipt: VISA"), "{reply}");
    assert!(reply.contains("1111"), "{reply}");
}

/// Full-width digits (U+FF10..U+FF19) for a run of ASCII digits.
fn full_width(s: &str) -> String {
    s.chars().map(|c| char::from_u32(c as u32 - '0' as u32 + 0xFF10).unwrap()).collect()
}

/// B3: card digits separated by NBSP or thin spaces, written full-width, or with wide gaps are masked too.
#[test]
fn mask_cards_sees_unicode_spaces_full_width_and_wide_gaps() {
    assert_eq!(
        insight::mask_cards("VISA 4111\u{a0}1111\u{a0}1111\u{a0}1111"),
        "VISA ****\u{a0}****\u{a0}****\u{a0}1111"
    );
    assert_eq!(
        insight::mask_cards("4111\u{2009}1111\u{2009}1111\u{2009}1111"),
        "****\u{2009}****\u{2009}****\u{2009}1111"
    );
    let fw = full_width("4111111111111111");
    assert_eq!(insight::mask_cards(&fw), format!("{}{}", "*".repeat(12), full_width("1111")));
    assert_eq!(insight::mask_cards("VISA 4111         1111 1111 1111"), "VISA ****         **** **** 1111");
    // The protected shapes still pass through unchanged.
    for same in ["Total $42.10        2026", "2026-10-13      14:03", "Phone 631-555-0100", "Lunch $12.90  Tip $2.00"] {
        assert_eq!(insight::mask_cards(same), same);
    }
}
