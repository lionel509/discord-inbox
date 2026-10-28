//! #130 round 6 (plan amendments 3 and 4): a card number stays masked whatever separator sits between its digit
//! groups, and his dotted prices, dates, IPs and versions come through untouched. `insight::mask_cards` and
//! `redact::sensitive` must agree on every row.

use discord_inbox::{insight, redact};

/// Both masking entry points on one input.
fn both(s: &str) -> (String, String) {
    (insight::mask_cards(s), redact::sensitive(s))
}

/// Asserts that `mask_cards` and `redact::sensitive` both give `want` for `s`.
fn same(s: &str, want: &str) {
    let (a, b) = both(s);
    assert_eq!(a, want, "mask_cards on {s:?}");
    assert_eq!(b, want, "redact::sensitive on {s:?}");
}

/// A small deterministic generator, the same LCG as `redact_r5.rs`.
fn lcg(seed: u64) -> impl FnMut() -> u64 {
    let mut x = seed;
    move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        x >> 33
    }
}

#[test]
fn card_layouts_are_masked() {
    let rows: [(&str, &str); 18] = [
        ("4111.1111.1111.1111 chicken", "****.****.****.1111 chicken"),
        ("4111.1111.1111.1111 5", "****.****.****.1111 5"),
        ("4111.1111.1111.1111 12.99", "****.****.****.1111 12.99"),
        ("12.99 4111.1111.1111.1111", "12.99 ****.****.****.1111"),
        ("4111-1111.1111-1111 12.99", "****-****.****-1111 12.99"),
        ("4111\u{200B}1111.1111\u{200B}1111 12.99", "****\u{200B}****.****\u{200B}1111 12.99"),
        ("4111\u{200E}1111\u{200E}1111\u{200E}1111 chicken", "****\u{200E}****\u{200E}****\u{200E}1111 chicken"),
        ("4111\u{2063}1111\u{2063}1111\u{2063}1111 chicken", "****\u{2063}****\u{2063}****\u{2063}1111 chicken"),
        ("4111\u{AD}1111\u{AD}1111\u{AD}1111", "****\u{AD}****\u{AD}****\u{AD}1111"),
        ("3782.822463.10005", "****.******.*0005"),
        ("3056.930902.5904", "****.******.5904"),
        ("4222.2222.2222.2", "****.****.*222.2"),
        ("4222.2222.2222.2 12.99", "****.****.*222.2 12.99"),
        ("4111.1111.1111.1111.110", "****.****.****.***1.110"),
        ("4111.1111.1111.1112 12.99", "****.****.****.1112 12.99"),
        ("4111.2222.2222.2222.2222", "****.****.****.****.2222"),
        ("4111.2222.2222.2222.2222 chicken", "****.****.****.****.2222 chicken"),
        ("12.99 4111 1111 1111 1111", "12.** **** **** **** 1111"),
    ];
    for (s, want) in rows {
        same(s, want);
    }
}

#[test]
fn dotted_prices_dates_ips_versions_stay() {
    let rows = [
        "12.99 4.50 3.00 7.25 1.99 10.00",
        "12.34.56.78 90.12.34.56",
        "192.168.1.10 10.0.0.255",
        "2026.10.27 2026.10.28 2026.10.29",
        "v2.13.1 v10.4.22",
        "1.2.3.4 5.6.7.8 9.10.11.12",
        "1234.56 7890.12 3456.78",
        "4006.3810.1234.5",
        "4111.2222.2226",
        "4111.2222.2226 12.99",
        "1234.5678.9012",
        "12.99",
        "1.5 kg",
        "2026.10.27",
        "192.168.100.200",
    ];
    for s in rows {
        same(s, s);
    }
}

#[test]
fn receipt_ocr_keeps_prices() {
    let input = "GREEN LEAF MARKET\n2026.10.27 18:42\nLANE 3 TERMINAL 10.0.4.17 v2.13.1\nBANANAS 1.29\nCHKN BRST 12.99\nJASMINE RICE 4.50\nOAT MILK 3.00\nSPINACH 7.25\nYELLOW ONION 1.99\nTOFU 2 @ 5.00 10.00\nSUBTOTAL 41.02\nTAX 0.00\nTOTAL 41.02\nVISA 4111.1111.1111.1111\nTHANK YOU\n";
    let want = input.replace("VISA 4111.1111.1111.1111", "VISA ****.****.****.1111");
    same(input, &want);
}

#[test]
fn card_matrix_every_separator() {
    // (digits, group lengths) for a 13, 14, 15, 16 and 19 digit card.
    let cards: [(&str, &[usize]); 5] = [
        ("4222222222222", &[4, 4, 4, 1]),
        ("30569309025904", &[4, 6, 4]),
        ("378282246310005", &[4, 6, 5]),
        ("4111111111111111", &[4, 4, 4, 4]),
        ("4111111111111111110", &[4, 4, 4, 4, 3]),
    ];
    let seps: [&str; 26] = [
        " ",
        "\u{A0}",
        "\t",
        "\n",
        "-",
        "\u{2013}",
        "\u{2014}",
        ".",
        "\u{200B}",
        "\u{200E}",
        "\u{AD}",
        "\u{2063}",
        "\u{FEFF}",
        ".\u{200B}",
        "/",
        ":",
        ",",
        "|",
        "_",
        "\u{FF0E}",
        "\u{FE0F}",
        "\u{301}",
        "\u{34F}",
        "\u{3164}",
        " / ",
        ", ",
    ];
    let prefixes = ["", "12.99 "];
    let suffixes = ["", " 12.99", " chicken", " 5", ".5"];
    for (digits, layout) in cards {
        let mut parts: Vec<&str> = Vec::new();
        let mut at = 0;
        for &n in layout {
            parts.push(&digits[at..at + n]);
            at += n;
        }
        assert_eq!(at, digits.len(), "layout of {digits}");
        for sep in seps {
            let card = parts.join(sep);
            let card_len = card.chars().count();
            for pre in prefixes {
                for suf in suffixes {
                    let s = format!("{pre}{card}{suf}");
                    let start = pre.chars().count();
                    let (a, b) = both(&s);
                    for out in [&a, &b] {
                        assert_eq!(out.chars().count(), s.chars().count(), "char count for {s:?} -> {out:?}");
                        let visible = out.chars().skip(start).take(card_len).filter(|c| c.is_ascii_digit()).count();
                        assert!(visible <= 4, "{s:?} shows {visible} digits of the card: {out:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn twelve_digit_dotted_stays_twenty_masked() {
    same("4111.2222.2226", "4111.2222.2226");
    same("4111.2222.2222.2222.2222", "****.****.****.****.2222");
}

#[test]
fn property_dotted_lists_never_masked() {
    let mut next = lcg(0x1306);
    for _ in 0..3000 {
        let k = 1 + next() % 12;
        let mut tokens: Vec<String> = Vec::new();
        for _ in 0..k {
            let kind = next() % 4;
            let tok = match kind {
                0 => {
                    let whole = 1 + next() % 9999;
                    let cents = next() % 100;
                    format!("{whole}.{cents:02}")
                }
                1 => {
                    let year = 1990 + next() % 110;
                    let month = 1 + next() % 12;
                    let day = 1 + next() % 31;
                    format!("{year}.{month:02}.{day:02}")
                }
                2 => {
                    let octets: Vec<String> = (0..4).map(|_| (next() % 256).to_string()).collect();
                    octets.join(".")
                }
                _ => {
                    let parts = 2 + next() % 3;
                    let nums: Vec<String> = (0..parts).map(|_| (next() % 100).to_string()).collect();
                    nums.join(".")
                }
            };
            tokens.push(tok);
        }
        let joiner = if next() % 2 == 0 { " " } else { "\n" };
        let s = tokens.join(joiner);
        assert_eq!(insight::mask_cards(&s), s, "dotted list was masked: {s:?}");
    }
}

#[test]
fn i3_card_with_non_gap_separator_masks() {
    for sep in ["/", ":", ",", "|", "_", "\u{FF0E}", "\u{30FB}", "\u{B7}", "+", "#"] {
        let s = format!("4111{sep}1111{sep}1111{sep}1111");
        same(&s, &format!("****{sep}****{sep}****{sep}1111"));
    }
}

#[test]
fn i3_card_with_invisible_mark_separator_masks() {
    for sep in ["\u{FE0F}", "\u{301}", "\u{3164}", "\u{34F}"] {
        let s = format!("4111{sep}1111{sep}1111{sep}1111");
        same(&s, &format!("****{sep}****{sep}****{sep}1111"));
    }
}

#[test]
fn spaced_punctuation_cards_mask() {
    same("4111 / 1111 / 1111 / 1111", "**** / **** / **** / 1111");
    same("4111, 1111, 1111, 1111", "****, ****, ****, 1111");
}

#[test]
fn mark_inside_a_group_still_masks() {
    same("4\u{301}111 1111 1111 1111", "*\u{301}*** **** **** 1111");
}

#[test]
fn punctuation_lists_that_are_not_cards_stay() {
    for s in [
        "10:30, 11:45, 12:00",
        "1,234,567,890,123",
        "631-555-1234, 516-555-6789",
        "+1 (631) 555-1234",
        "10/27/2026 2026/10/28 18:42:05",
    ] {
        same(s, s);
    }
    // Four 4-digit numbers joined by punctuation are masked on purpose: a card layout cannot be told
    // apart from them, and the same list joined by spaces is already masked.
    assert_eq!(insight::mask_cards("2024, 2025, 2026, 2027"), "****, ****, ****, 2027");
}

#[test]
fn property_mixed_lists_never_masked() {
    let mut next = lcg(0x1307);
    for _ in 0..3000 {
        let k = 1 + next() % 12;
        let mut tokens: Vec<String> = Vec::new();
        for _ in 0..k {
            let kind = next() % 7;
            let tok = match kind {
                0 => {
                    let whole = 1 + next() % 9999;
                    let cents = next() % 100;
                    format!("{whole}.{cents:02}")
                }
                1 => {
                    let year = 1990 + next() % 110;
                    let month = 1 + next() % 12;
                    let day = 1 + next() % 31;
                    format!("{year}.{month:02}.{day:02}")
                }
                2 => {
                    let octets: Vec<String> = (0..4).map(|_| (next() % 256).to_string()).collect();
                    octets.join(".")
                }
                3 => {
                    let parts = 2 + next() % 3;
                    let nums: Vec<String> = (0..parts).map(|_| (next() % 100).to_string()).collect();
                    nums.join(".")
                }
                4 => {
                    let h = next() % 24;
                    let m = next() % 60;
                    format!("{h:02}:{m:02}")
                }
                5 => {
                    let month = 1 + next() % 12;
                    let day = 1 + next() % 31;
                    let year = 1990 + next() % 110;
                    format!("{month}/{day}/{year}")
                }
                _ => {
                    let whole = 1 + next() % 999;
                    let thousands = next() % 1000;
                    let cents = next() % 100;
                    format!("${whole},{thousands:03}.{cents:02}")
                }
            };
            tokens.push(tok);
        }
        let mut s = String::new();
        for (n, tok) in tokens.iter().enumerate() {
            if n > 0 {
                let joiners = [" ", "\n", ", ", " / ", " | "];
                s.push_str(joiners[(next() % 5) as usize]);
            }
            s.push_str(tok);
        }
        assert_eq!(insight::mask_cards(&s), s, "list was masked: {s:?}");
    }
}
