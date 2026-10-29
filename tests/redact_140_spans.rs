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

// #141 rework r1 breaker: a label swallowed by a token run still has its value redacted.
#[test]
fn bearer_digits_then_dot_password_label_hides_value() {
    let out = sensitive("Bearer 12345678901234.password: hunter22");
    assert_eq!(out, "Bearer [redacted]: [redacted]");
    assert!(!out.contains("hunter22"));
}

#[test]
fn bearer_card_then_dot_token_label_hides_value() {
    let out = sensitive("Bearer 4111111111111111.token: hunter22");
    assert_eq!(out, "Bearer [redacted]: [redacted]");
    assert!(!out.contains("hunter22"));
}

#[test]
fn basic_card_then_tilde_password_is_hides_value() {
    let out = sensitive("Authorization: Basic 4111111111111111~~password is hunter22");
    assert_eq!(out, "Authorization: Basic [redacted] is [redacted]");
    assert!(!out.contains("hunter22"));
}

#[test]
fn bearer_ssn_then_dot_password_label_hides_value() {
    let out = sensitive("Bearer 123-45-6789.password: hunter22");
    assert_eq!(out, "Bearer [redacted]: [redacted]");
    assert!(!out.contains("hunter22"));
}

#[test]
fn bearer_letters_then_dot_password_label_hides_value() {
    let out = sensitive("Bearer abcdefghijklmnopqrst.password: hunter22");
    assert_eq!(out, "Bearer [redacted]: [redacted]");
    assert!(!out.contains("hunter22"));
}

#[test]
fn labels_inside_long_token_run_are_linear() {
    let input = format!("Bearer {}x", ".password=".repeat(20_000));
    let t = Instant::now();
    let _ = sensitive(&input);
    assert!(t.elapsed().as_secs_f64() < 1.0, "took {:?}", t.elapsed());
}

// #141 rework r2 breaker: a label inside a token never reaches past the token; a label ending at the token's end is a stand-alone label.
#[test]
fn bold_wrapped_bearer_label_keeps_closing_markup() {
    let out = sensitive("**Bearer abcdefghijklmnopqrst.password=hunter22**");
    assert_eq!(out, "**Bearer [redacted]**");
    assert!(!out.contains("hunter22"));
}

#[test]
fn backtick_wrapped_bearer_label_keeps_closing_backtick() {
    assert_eq!(sensitive("`Bearer abcdefghijklmnopqrst.token=xyz`"), "`Bearer [redacted]`");
}

#[test]
fn parenthesised_bearer_label_keeps_closing_paren() {
    assert_eq!(sensitive("(Bearer abcdefghijklmnopqrst.token=xyz) done"), "(Bearer [redacted]) done");
}

#[test]
fn json_bearer_label_keeps_rest_of_line() {
    assert_eq!(
        sensitive(r#"{"auth":"Bearer abcdefghijklmnopqrst.token=xyz","n":1}"#),
        r#"{"auth":"Bearer [redacted]","n":1}"#
    );
}

#[test]
fn url_userinfo_label_keeps_host() {
    assert_eq!(sensitive("https://user:token=abc@example.com/x"), "https://user:[redacted]@example.com/x");
}

#[test]
fn bold_bearer_empty_in_run_value_keeps_markup() {
    assert_eq!(sensitive("**Bearer abcdefghijklmnopqrst.token=**"), "**Bearer [redacted]**");
}

#[test]
fn url_userinfo_password_label_keeps_host() {
    let out = sensitive("https://user:password:hunter22@example.com/x");
    assert_eq!(out, "https://user:[redacted]@example.com/x");
    assert!(!out.contains("hunter22"));
}

#[test]
fn two_labels_in_bearer_run_hide_last_value() {
    let out = sensitive("Bearer abcdefghijklmnopqrst.token=x.password: hunter22");
    assert_eq!(out, "Bearer [redacted]: [redacted]");
    assert!(!out.contains("hunter22"));
}

#[test]
fn basic_label_at_kept_hides_value() {
    let out = sensitive("Authorization: Basic password: hunter22");
    assert_eq!(out, "Authorization: Basic [redacted]: [redacted]");
    assert!(!out.contains("hunter22"));
}

#[test]
fn bold_bearer_label_after_run_redacts_to_whitespace() {
    let out = sensitive("**Bearer abcdefghijklmnopqrst.password: hunter22**");
    assert_eq!(out, "**Bearer [redacted]: [redacted]");
    assert!(!out.contains("hunter22"));
}

#[test]
fn url_userinfo_shorter_than_a_label_does_not_panic() {
    assert_eq!(sensitive("a://:pw@h"), "a://:[redacted]@h");
}

#[test]
fn labels_ending_each_token_are_linear() {
    let input = "Bearer abcdefghijklmnopqrst.password: x ".repeat(20_000);
    let t = Instant::now();
    let _ = sensitive(&input);
    assert!(t.elapsed().as_secs_f64() < 2.0, "took {:?}", t.elapsed());
}
