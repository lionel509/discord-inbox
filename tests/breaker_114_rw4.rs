//! Breaker, final fix round on PR #114 at head 2f2ee71, plus the reviewer's pins (review 5661154129).
//! Attacks the `~` -> space replacement in `nutrition::scan`. Made-up fixtures only.
use discord_inbox::nutrition;

fn logged(p: &nutrition::Parsed) -> Vec<(&'static str, f64, bool)> {
    p.items.iter().map(|i| (i.food.name, i.grams, i.assumed)).collect()
}

/// B1: a tilde range after "at" is read as a clock time. "salmon at 5-6 oz" is refused, but
/// "salmon at 5~6 oz" silently logs 6 oz (170.1 g): the 5 is dropped as an hour and the range is lost.
#[test]
fn tilde_range_after_at_is_not_silently_a_time() {
    let dash = nutrition::parse_meal("salmon at 5-6 oz");
    let tilde = nutrition::parse_meal("salmon at 5~6 oz");
    assert!(
        dash.items.is_empty() && !dash.unknown.is_empty(),
        "control: the dash range should be refused, got {:?}",
        logged(&dash)
    );
    assert!(
        tilde.items.is_empty() && !tilde.unknown.is_empty(),
        "salmon at 5~6 oz should be refused like 5-6, but logged {:?}",
        logged(&tilde)
    );
}

/// B2: the same class with grams. "at 1~200 g" drops the 1 as a time and logs 200 g of oats.
#[test]
fn tilde_range_of_grams_after_at_is_not_silently_a_time() {
    let tilde = nutrition::parse_meal("oats at 1~200 g");
    assert!(
        tilde.items.is_empty() && !tilde.unknown.is_empty(),
        "oats at 1~200 g should be refused (two amounts), but logged {:?}",
        logged(&tilde)
    );
}

/// Reviewer pin: "around" behaves like "at" for a tilde range. "salmon around 5~6 oz" must be refused.
#[test]
fn tilde_range_after_around_is_not_silently_a_time() {
    let tilde = nutrition::parse_meal("salmon around 5~6 oz");
    assert!(
        tilde.items.is_empty() && !tilde.unknown.is_empty(),
        "salmon around 5~6 oz should be refused, but logged {:?}",
        logged(&tilde)
    );
}

/// B3: a full-width tilde (U+FF5E) before an amount is not handled like the ASCII one. "oats ～150 g"
/// logs the 40 g default (assumed) and the 150 g is lost.
#[test]
fn fullwidth_tilde_before_amount_is_the_amount() {
    let p = nutrition::parse_meal("oats ～150 g");
    assert!(p.items.iter().any(|i| (i.grams - 150.0).abs() < 0.5), "oats ～150 g logged {:?}", logged(&p));
}

/// Guard: "1~2 eggs" is an ambiguous range, so it must be refused, not logged as 2 eggs.
#[test]
fn guard_tilde_range_is_refused() {
    let p = nutrition::parse_meal("1~2 eggs");
    assert!(p.items.is_empty() && !p.unknown.is_empty(), "1~2 eggs logged {:?}", logged(&p));
}

/// Guard: a tilde before an amount is "about", and the amount stays.
#[test]
fn guard_tilde_before_amount_keeps_amount() {
    let p = nutrition::parse_meal("oats ~150 g");
    assert!(p.items.iter().any(|i| (i.grams - 150.0).abs() < 0.5), "oats ~150 g logged {:?}", logged(&p));
    let p = nutrition::parse_meal("oats ~1 1/2 cup");
    assert!(p.items.iter().any(|i| (i.grams - 121.5).abs() < 0.5), "oats ~1 1/2 cup logged {:?}", logged(&p));
}

/// Guard: a tilde clock time ("at ~7") stays a time, so the 150 g after it is the amount.
#[test]
fn guard_tilde_clock_time_keeps_amount() {
    let p = nutrition::parse_meal("salmon at ~7 150 g");
    assert!(p.items.iter().any(|i| (i.grams - 150.0).abs() < 0.5), "salmon at ~7 150 g logged {:?}", logged(&p));
}

/// Guard: a tilde after an amount ("150~ g") is "about", so the 150 g is kept.
#[test]
fn guard_tilde_after_amount_keeps_amount() {
    let p = nutrition::parse_meal("oats 150~ g");
    assert!(p.items.iter().any(|i| (i.grams - 150.0).abs() < 0.5), "oats 150~ g logged {:?}", logged(&p));
}

/// Guard: a tilde before a thousands group is "about", so 1,000 g is 1000 g.
#[test]
fn guard_tilde_before_thousands_keeps_amount() {
    let p = nutrition::parse_meal("oats ~1,000 g");
    assert!(p.items.iter().any(|i| (i.grams - 1000.0).abs() < 0.5), "oats ~1,000 g logged {:?}", logged(&p));
}

/// Guard: a full-width tilde after an amount is either 150 g or refused, never another amount.
#[test]
fn guard_fullwidth_tilde_after_amount_is_150_or_refused() {
    let p = nutrition::parse_meal("oats 150～g");
    let ok = p.items.iter().all(|i| (i.grams - 150.0).abs() < 0.5) && (!p.items.is_empty() || !p.unknown.is_empty());
    assert!(ok, "oats 150～g logged {:?}", logged(&p));
}
