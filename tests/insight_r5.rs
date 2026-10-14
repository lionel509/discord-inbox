use discord_inbox::insight;

#[test]
fn parse_ignores_ansi_escapes() {
    let s = "\x1b[?25lreceipt\x1b[?25h:\x1b[?25l yes\n\x1b[1Gmerchant: Trader Joe's\ntotal: 12.49\ncategory: other\n";
    let i = insight::parse(s).unwrap();
    assert!(i.receipt);
    assert_eq!(i.total, "12.49");
}

#[test]
fn masks_en_dash_and_wide_gap() {
    let a = insight::mask_cards("Card 4111\u{2013}1111\u{2013}1111\u{2013}1111");
    assert!(!a.contains("4111") && a.ends_with("1111"));
    let b = insight::mask_cards("VISA    4111    1111    1111    1111");
    assert!(!b.contains("4111") && b.ends_with("1111"));
}
