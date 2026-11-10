//! #159 U1: the receipt verdict counts a store app's purchase-history screen as a receipt (prompt and rubric).
use discord_inbox::insight;

const HISTORY: &str = "Order history\nNov 8, 2026\nFarmhouse Sourdough Bread 24 oz $3.49\nOrder total $3.49";

#[test]
fn prompt_counts_purchase_history() {
    let p = insight::prompt(HISTORY);
    assert!(p.contains("purchase-history screen"), "{p}");
}

#[test]
fn rubric_counts_purchase_history() {
    let body = insight::scaledown_request(HISTORY);
    assert!(body.contains("purchase-history screen"), "{body}");
}
