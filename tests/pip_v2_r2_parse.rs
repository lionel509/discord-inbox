//! Round 2 parse fixes on PR #114 (review R1): strict unitless numbers, egg as a food, "eggs" as a plural,
//! time phrases dropped before the amount, compound lb+oz, and the second amount refused.
use discord_inbox::nutrition::{self, Unit};

fn err_text(piece: &str) -> String {
    match nutrition::parse_line(piece) {
        Ok(it) => panic!("{piece:?} should be refused, got {} g {}", it.grams, it.food.name),
        Err(u) => format!("{} {}", u.text, u.hint),
    }
}

#[test]
fn bare_numbers_without_a_unit_are_refused_not_grams() {
    for piece in ["salmon 7", "salmon 150", "200 cooked rice"] {
        assert!(err_text(piece).contains("no unit"), "{piece:?}: {}", err_text(piece));
    }
}

#[test]
fn egg_is_a_food_and_counts_as_pieces() {
    let it = nutrition::parse_line("2 eggs").unwrap_or_else(|u| panic!("unknown: {} ({})", u.text, u.hint));
    assert_eq!(it.food.name, "egg");
    assert_eq!(it.unit, Unit::Piece);
    assert!((it.qty - 2.0).abs() < 1e-9);
    assert!((it.grams - 100.0).abs() < 0.01, "got {}", it.grams);

    let large = nutrition::parse_line("2 large egg").unwrap_or_else(|u| panic!("unknown: {} ({})", u.text, u.hint));
    assert_eq!(large.food.name, "egg");
    assert_eq!(large.unit, Unit::Piece);
    assert!((large.grams - 100.0).abs() < 0.01, "got {}", large.grams);
}

#[test]
fn one_banana_is_one_piece() {
    let it = nutrition::parse_line("1 banana").unwrap_or_else(|u| panic!("unknown: {} ({})", u.text, u.hint));
    assert_eq!(it.unit, Unit::Piece);
    assert!((it.grams - 118.0).abs() < 0.01, "got {}", it.grams);
}

#[test]
fn second_amount_in_one_line_is_refused() {
    let e = err_text("salmon 150g 2");
    assert!(e.contains("two amounts"), "{e}");
}

#[test]
fn clock_time_phrase_before_the_food_or_after_it_is_dropped() {
    let it = nutrition::parse_line("salmon at 7").unwrap_or_else(|u| panic!("unknown: {} ({})", u.text, u.hint));
    assert_eq!(it.food.name, "salmon");
    assert!(it.assumed, "salmon at 7 should be the default portion");
}

#[test]
fn eggs_on_toast_names_two_foods() {
    let p = nutrition::parse_meal("2 eggs on toast");
    assert!(p.items.is_empty(), "no item should be logged: {:?}", p.items.iter().map(|i| (i.food.name, i.grams)).collect::<Vec<_>>());
    assert!(p.unknown.first().is_some_and(|u| u.hint.contains("two foods")), "{:?}", p.unknown.iter().map(|u| (&u.text, &u.hint)).collect::<Vec<_>>());
}

#[test]
fn small_talk_with_a_clock_time_is_not_food() {
    for text in ["dinner at 7", "lunch at 12:30pm", "meet at 9am"] {
        assert!(!nutrition::looks_like_food(text), "{text:?} read as food");
    }
}
