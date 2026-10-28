use discord_inbox::redact::sensitive;

// #141: the label rules (password, token) must see a spaced card whole, not only its first group.
#[test]
fn amex_grouped_card_after_password_label_is_starred() {
    assert_eq!(sensitive("password: 3782 822463 10005"), "password: [redacted] ****** *0005");
}

#[test]
fn amex_grouped_card_after_token_label_is_starred() {
    assert_eq!(sensitive("token=3782 822463 10005"), "token=[redacted] ****** *0005");
}

#[test]
fn thirteen_digit_card_after_token_label_is_starred() {
    assert_eq!(sensitive("token=4222 222 222 222"), "token=[redacted] *** **2 222");
}

// #141: a token run that holds a card and then ordinary words is one token. Pinned on purpose.
#[test]
fn bearer_value_with_card_then_words_is_redacted_whole() {
    assert_eq!(sensitive("Bearer 4111-1111-1111-1112-and-more-text-here"), "Bearer [redacted]");
}

#[test]
fn sk_value_with_card_then_words_is_redacted_whole() {
    assert_eq!(sensitive("sk-4111-1111-1111-1112-is-my-card"), "[redacted]");
}

// #141: Discord bold around a token must stay outside the redaction.
#[test]
fn bold_around_sk_token_keeps_the_stars_outside() {
    assert_eq!(sensitive("**sk-abcdefghijklmnop1234**"), "**[redacted]**");
}

// #141 round 2: a `*` joins a token only when it is part of a card the number rule starred. Bold ends the token.
#[test]
fn break_d_bold_closer_then_word_keeps_the_word() {
    assert_eq!(sensitive("**sk-abcdefghijklmnop1234**text"), "**[redacted]**text");
}

#[test]
fn break_e_token_then_bold_then_words_keeps_the_words() {
    assert_eq!(sensitive("sk-abcdefghijklmnop1234**and more"), "[redacted]**and more");
}

#[test]
fn break_f_bearer_star_then_letter_keeps_the_tail() {
    assert_eq!(sensitive("Bearer abcdefghijklmnopqrstuvw*x"), "Bearer [redacted]*x");
}

#[test]
fn bearer_dotted_card_inside_token_is_redacted_whole() {
    assert_eq!(sensitive("Authorization: Bearer x/4111.1111.1111.1112"), "Authorization: Bearer [redacted]");
}

#[test]
fn bold_card_inside_bearer_token_is_redacted_whole() {
    assert_eq!(sensitive("Authorization: Bearer x/**4111-1111-1111-1112**"), "Authorization: Bearer [redacted]**");
}
