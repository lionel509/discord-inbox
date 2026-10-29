use discord_inbox::redact::sensitive;

#[test]
fn bearer_token_with_order_label_card_is_redacted_whole() {
    assert_eq!(
        sensitive("Authorization: Bearer x/order.4111-1111-1111-1112"),
        "Authorization: Bearer [redacted]"
    );
}

#[test]
fn bearer_token_with_receipt_label_card_is_redacted_whole() {
    assert_eq!(
        sensitive("Authorization: Bearer x/receipt.4111-1111-1111-1112"),
        "Authorization: Bearer [redacted]"
    );
}

#[test]
fn bearer_token_with_plus_card_is_redacted_whole() {
    assert_eq!(
        sensitive("Authorization: Bearer /+4111-1111-1111-1112"),
        "Authorization: Bearer [redacted]"
    );
}

#[test]
fn unlabelled_card_inside_bearer_token_is_redacted_whole() {
    assert_eq!(
        sensitive("Authorization: Bearer x/4111-1111-1111-1112"),
        "Authorization: Bearer [redacted]"
    );
}
