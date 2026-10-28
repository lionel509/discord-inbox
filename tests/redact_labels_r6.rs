//! #130 unit B (store labels, #39): a digit run grouped like a card (four 4-digit groups in a row) is never kept
//! as a store reference after a label or `+`, even when Luhn fails. Unspaced runs and real references still stay.

use discord_inbox::redact::sensitive;

#[test]
fn store_label_never_keeps_a_card_grouped_run() {
    assert_eq!(sensitive("order 4111 1111 1111 1112"), "order **** **** **** 1112");
    assert_eq!(sensitive("ref 4111 1111 1111 1112"), "ref **** **** **** 1112");
    assert_eq!(sensitive("receipt 4111 1111 1111 1112"), "receipt **** **** **** 1112");
    assert_eq!(sensitive("+4111 1111 1111 1112"), "+**** **** **** 1112");
    assert_eq!(sensitive("order 4111-1111-1111-1112"), "order ****-****-****-1112");
    assert_eq!(sensitive("txn 4111 1111 1111 1112 5"), "txn **** **** **** *112 5");
}

#[test]
fn store_label_still_keeps_references() {
    assert_eq!(sensitive("Order ID: 113-1234567-1234567"), "Order ID: 113-1234567-1234567");
    assert_eq!(sensitive("Phone +86 138 0013 8000"), "Phone +86 138 0013 8000");
    assert_eq!(sensitive("receipt 1234 5678 9012"), "receipt 1234 5678 9012");
    assert_eq!(sensitive("order 1234567890123456"), "order 1234567890123456");
    assert_eq!(sensitive("ref 3782 822463 10006"), "ref 3782 822463 10006");
}
