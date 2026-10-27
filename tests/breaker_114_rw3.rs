//! Breaker round 3 on PR #114 at head c031cf6. Attacks the r2 fix commits 28291f7..c031cf6
//! (mixed numbers after around/at, " and " recipe titles, and the send_each / checkin doc text).
//! Made-up fixtures only, in the system temp dir.
use discord_inbox::nutrition;

/// B3: a tilde before an amount is not a time, but the amount is lost: "~150 g" logs the 40 g default
/// (assumed) and "~1 1/2 cup" logs only the 1/2 cup. Oats are 81 g per cup, so 1 1/2 cup is 121.5 g.
#[test]
fn tilde_before_an_amount_is_not_dropped() {
    let p = nutrition::parse_meal("oats ~150 g");
    assert!(p.items.iter().any(|i| (i.grams - 150.0).abs() < 0.5), "oats ~150 g logged {:?}", p.items.iter().map(|i| (i.food.name, i.grams, i.assumed)).collect::<Vec<_>>());
    let p = nutrition::parse_meal("oats ~1 1/2 cup");
    assert!(p.items.iter().any(|i| (i.grams - 121.5).abs() < 0.5), "oats ~1 1/2 cup logged {:?}", p.items.iter().map(|i| (i.food.name, i.grams, i.assumed)).collect::<Vec<_>>());
}
