//! Breaker round 2 on PR #114 at head 5c3c36b (parse + recipes). Each test pins one break the
//! fix commits f42df1c..5c3c36b still have. Made-up fixtures only, in the system temp dir.
use discord_inbox::nutrition;

/// B1: "2 eggs on toast" logs two slices of bread (60 g) and drops the eggs without an unknown.
#[test]
fn eggs_on_toast_does_not_drop_the_eggs() {
    let p = nutrition::parse_meal("2 eggs on toast");
    assert!(
        p.items.iter().any(|i| i.food.name == "egg") || !p.unknown.is_empty(),
        "eggs silently dropped: items={:?}",
        p.items.iter().map(|i| (i.food.name, i.grams)).collect::<Vec<_>>()
    );
}

/// B2: "salmon at 7" reads the bare 7 as seven pieces: 1050 g of salmon, logged silently.
#[test]
fn salmon_at_7_is_not_seven_pieces() {
    let p = nutrition::parse_meal("salmon at 7");
    assert!(
        !p.items.iter().any(|i| i.grams > 500.0),
        "salmon at 7 logged {:?}",
        p.items.iter().map(|i| (i.food.name, i.grams)).collect::<Vec<_>>()
    );
}

/// B3: a clock time after the food makes the whole piece unknown, so the food is dropped.
#[test]
fn food_with_a_clock_time_phrase_is_still_logged() {
    for text in ["2 eggs at 8am", "Had salmon at 7pm"] {
        let p = nutrition::parse_meal(text);
        assert!(!p.items.is_empty(), "{text:?} lost its food: unknown={:?}", p.unknown.iter().map(|u| (&u.text, &u.hint)).collect::<Vec<_>>());
    }
}

/// B4: "heads" is in the head check but not in the unit words, so the plural is rejected.
#[test]
fn plural_heads_of_cabbage_is_1800_g() {
    let it = nutrition::parse_line("2 heads cabbage").unwrap_or_else(|u| panic!("unknown: {} ({})", u.text, u.hint));
    assert!((it.grams - 1800.0).abs() < 0.5, "got {}", it.grams);
}

/// B5: "fl oz" (a standard US volume unit) is rejected as an unknown unit.
#[test]
fn fluid_ounces_are_read_as_ounces() {
    let it = nutrition::parse_line("8 fl oz milk").unwrap_or_else(|u| panic!("unknown: {} ({})", u.text, u.hint));
    assert!((200.0..=260.0).contains(&it.grams), "got {}", it.grams);
}

/// B6: "2 lb 4 oz salmon" keeps the 2 lb and drops the 4 oz (907 g, want 1020 g), with no unknown.
#[test]
fn compound_pounds_and_ounces_add_up() {
    let it = nutrition::parse_line("2 lb 4 oz salmon").unwrap_or_else(|u| panic!("unknown: {} ({})", u.text, u.hint));
    assert!((it.grams - 1020.1).abs() < 1.0, "got {}", it.grams);
}
