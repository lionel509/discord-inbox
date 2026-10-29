use discord_inbox::redact::sensitive;
use std::time::Instant;

// #141 round 3: markdown after a token stays as main writes it.
#[test]
fn bold_closer_then_digit_keeps_closer_and_digit() {
    assert_eq!(sensitive("**sk-abcdefghijklmnop1234**2 apples"), "**[redacted]**2 apples");
}

#[test]
fn token_then_star_then_digit_word_keeps_the_word() {
    assert_eq!(sensitive("sk-abcdefghijklmnop1234**3rd place"), "[redacted]**3rd place");
}

#[test]
fn bearer_then_bold_closer_then_date_keeps_the_date() {
    assert_eq!(sensitive("**Bearer abcdefghijklmnopqrstuvw**2026-10-28 meeting"), "**Bearer [redacted]**2026-10-28 meeting");
}

#[test]
fn bold_closer_then_underscore_digit_keeps_the_text() {
    assert_eq!(sensitive("**sk-abcdefghijklmnop1234**_2 units"), "**[redacted]**_2 units");
}

#[test]
fn bold_closer_dash_bold_keeps_the_markup() {
    assert_eq!(sensitive("**sk-abcdefghijklmnop1234**-**bold** tail"), "**[redacted]**-**bold** tail");
}

#[test]
fn single_star_digit_after_bearer_keeps_the_star() {
    assert_eq!(sensitive("Bearer abcdefghijklmnopqrstuvw*2 more"), "Bearer [redacted]*2 more");
}

#[test]
fn bold_closer_then_dot_digit_keeps_the_text() {
    assert_eq!(sensitive("**sk-abcdefghijklmnop1234**.5 units"), "**[redacted]**.5 units");
}

#[test]
fn bold_closer_then_slash_digit_keeps_the_text() {
    assert_eq!(sensitive("**sk-abcdefghijklmnop1234**/2 units"), "**[redacted]**/2 units");
}

// #140 rework: spans found on the original text, merged, written once.
#[test]
fn card_straddles_end_of_bearer_token() {
    assert_eq!(sensitive("Bearer abcdefghijklmnopqrst4111 1111 1111 1112"), "Bearer [redacted] **** **** 1112");
}

#[test]
fn card_straddles_start_of_url_password() {
    assert_eq!(sensitive("https://4111-1111-1111:1112@example.com/x"), "https://****-****-****:[redacted]@example.com/x");
}

#[test]
fn labelled_ssn_overlapping_token_value_is_redacted_whole() {
    assert_eq!(sensitive("token=SSN:123 45 6789"), "token=[redacted]");
}

#[test]
fn labelled_ssn_inside_card_run_shows_no_digit() {
    assert_eq!(sensitive("SSN 123456789 1234"), "SSN ***-**-**** 1234");
}

#[test]
fn card_and_ssn_overlap_show_only_last_four() {
    assert_eq!(sensitive("123-45-6789 1234"), "***-**-**** 1234");
}

#[test]
fn secret_then_ssn_with_one_space() {
    assert_eq!(sensitive("password=hunter22 123-45-6789"), "password=[redacted] ***-**-6789");
}

#[test]
fn two_secrets_back_to_back() {
    assert_eq!(sensitive("token=abcdefgh1 password=hunter22 ok"), "token=[redacted] password=[redacted] ok");
}

#[test]
fn bold_wrapped_bearer_with_card_keeps_bold() {
    assert_eq!(sensitive("**Bearer x/order.4111-1111-1111-1112**"), "**Bearer [redacted]**");
}

// #140 rework, constraint (e): one forward scan per rule.
#[test]
fn star_run_after_bearer_is_linear() {
    let input = format!("Bearer {}1 x", "*".repeat(50_000));
    let t = Instant::now();
    let _ = sensitive(&input);
    assert!(t.elapsed().as_secs_f64() < 1.0, "took {:?}", t.elapsed());
}

const PRE: &[&str] = &["", "hi ", "**", "*", "||", "`", "~~", "(", "> "];
const SUF: &[&str] = &["", " tail", "**", "**text", "**and more", "**2 apples", "**3rd place",
    "**2026-10-28 meeting", "**_2 units", "**.5", "**/2", "*2 more", "*x", "**-**bold** tail", "||", "`", ")"];
fn digits(s: &str) -> usize {
    s.chars().filter(|c| c.is_ascii_digit()).count()
}

// Inputs built from the shapes real Discord messages and OCR produce.
#[test]
fn secrets_with_cards_show_at_most_last_four() {
    const CARDS: &[&str] = &["4111-1111-1111-1112", "4111.1111.1111.1112", "4111/1111/1111/1112", "4111 1111 1111 1112",
        "4111111111111112", "3782-822463-10005", "3782.822463.10005", "3782 822463 10005", "378282246310005",
        "4222 222 222 222", "4222-222-222-222"];
    const SHAPES: &[&str] = &["Authorization: Bearer x/{}", "Bearer x/order.{}", "Bearer abcdefghijklmnopqrst{}", "sk-{}",
        "xoxb-{}", "password: {}", "token={}", "pwd is {}", "https://{}:pw@example.com/x", "card {} token=abcdefgh"];
    for pre in PRE {
        for shape in SHAPES {
            for card in CARDS {
                for suf in SUF {
                    let input = format!("{pre}{}{suf}", shape.replace("{}", card));
                    let out = sensitive(&input);
                    assert!(digits(&out) <= digits(pre) + digits(suf) + 4, "{input:?} -> {out:?}");
                    assert_eq!(sensitive(&out), out, "not idempotent: {input:?}");
                }
            }
        }
    }
}

#[test]
fn plain_secrets_keep_surrounding_markup() {
    const SECRETS: &[(&str, &str)] = &[("sk-abcdefghijklmnopqrst", "[redacted]"), ("Bearer abcdefghijklmnopqrstuvw", "Bearer [redacted]"),
        ("Authorization: Bearer abcdefghij", "Authorization: Bearer [redacted]"), ("Authorization: Basic abcdefghij", "Authorization: Basic [redacted]"),
        ("ghp_abcdefghijklmnopqrst", "[redacted]"), ("xoxb-abcdefghijk", "[redacted]"), ("AKIAABCDEFGHIJKLMNOP", "[redacted]")];
    for pre in PRE {
        for (secret, want) in SECRETS {
            for suf in SUF {
                let input = format!("{pre}{secret}{suf}");
                let out = sensitive(&input);
                assert_eq!(out, format!("{pre}{want}{suf}"));
                assert_eq!(sensitive(&out), out, "not idempotent: {input:?}");
            }
        }
    }
}
