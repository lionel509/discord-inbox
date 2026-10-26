//! U1: one quantity grammar (number anywhere, `name:qty`, units after the food), word-order-free aliases, and `looks_like_food`.
use discord_inbox::nutrition::{self, Item, Unit};

fn one(piece: &str) -> Item {
    nutrition::parse_line(piece).unwrap_or_else(|u| panic!("{piece:?} unknown: {} ({})", u.text, u.hint))
}

#[test]
fn colon_form_with_fraction_and_cup_is_cooked_rice_79_g() {
    let it = one("Cooked white rice:1/2 cup");
    assert_eq!(it.food.name, "cooked rice");
    assert!((it.grams - 79.0).abs() < 0.01, "got {}", it.grams);
}

#[test]
fn grams_after_food_read_as_grams() {
    let rice = one("cooked rice 180g");
    assert_eq!(rice.food.name, "cooked rice");
    assert!((rice.grams - 180.0).abs() < 0.01);
    assert!((one("salmon 112 g").grams - 112.0).abs() < 0.01);
    assert!((one("cabbage 85g").grams - 85.0).abs() < 0.01);
}

#[test]
fn long_salmon_name_is_typed_not_assumed() {
    let it = one("Farmed Atlantic salmon 100 grams");
    assert_eq!(it.food.name, "salmon");
    assert!((it.grams - 100.0).abs() < 0.01);
    assert!(!it.assumed);
}

#[test]
fn clock_time_is_not_an_amount() {
    assert!((one("12:30 salmon 100g").grams - 100.0).abs() < 0.01);
}

#[test]
fn ambiguous_rice_is_still_unknown() {
    assert!(nutrition::parse_line("white rice").is_err());
}

#[test]
fn bare_count_of_a_piece_food_is_pieces() {
    let egg = one("2 large egg");
    assert_eq!(egg.unit, Unit::Piece);
    assert!((egg.grams - 100.0).abs() < 0.01, "got {}", egg.grams);
}

#[test]
fn bare_number_on_a_gram_food_is_grams_times_items() {
    let salmon = one("salmon 2");
    assert!((salmon.grams - 300.0).abs() < 0.01, "got {}", salmon.grams);
}

#[test]
fn looks_like_food_separates_chat_from_meals() {
    for chat in ["test", "hi pip", "thanks!"] {
        assert!(!nutrition::looks_like_food(chat), "{chat:?} should not look like food");
    }
    for food in ["rice", "samon 100g", "½ cup"] {
        assert!(nutrition::looks_like_food(food), "{food:?} should look like food");
    }
}

/// Fixed-seed LCG: no crate, same sequence every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[(self.next() as usize) % xs.len()]
    }
}

#[test]
fn property_pieces_round_trip_without_invention_or_loss() {
    // (alias, expected food name, cup grams when a cup form is allowed)
    let foods: [(&str, &str, Option<f64>); 5] = [
        ("cooked rice", "cooked rice", Some(158.0)),
        ("atlantic salmon", "salmon", None),
        ("cabbage", "cabbage", Some(89.0)),
        ("honey", "honey", Some(339.0)),
        ("peanut butter", "peanut butter", Some(258.0)),
    ];
    let qtys: [(&str, f64); 7] =
        [("1", 1.0), ("2.5", 2.5), ("85", 85.0), ("100", 100.0), ("180", 180.0), ("1/2", 0.5), ("½", 0.5)];
    let mut rng = Lcg(0x5EED_111);
    for case in 0..500 {
        let n = 1 + (rng.next() % 4) as usize;
        let mut pieces = Vec::new();
        let mut expect = Vec::new();
        for _ in 0..n {
            let (alias, name, cup) = *rng.pick(&foods);
            let (qs, q) = *rng.pick(&qtys);
            let mut units = vec!["g", "grams"];
            if cup.is_some() {
                units.push("cup");
            }
            let u = *rng.pick(&units);
            let per = if u == "cup" { cup.unwrap() } else { 1.0 };
            let form = rng.next() % 4;
            let piece = match form {
                0 => format!("{qs} {u} {alias}"),
                1 => format!("{alias} {qs} {u}"),
                2 => format!("{alias}:{qs} {u}"),
                _ => {
                    if u == "g" {
                        format!("{alias} {qs}{u}")
                    } else {
                        format!("{qs} {u} {alias}")
                    }
                }
            };
            pieces.push(piece);
            expect.push((name, q * per));
        }
        let sep = if rng.next() % 2 == 0 { "\n" } else { ", " };
        let text = pieces.join(sep);
        let parsed = nutrition::parse_meal(&text);
        assert!(parsed.unknown.is_empty(), "case {case}: {text:?} unknown {:?}", parsed.unknown);
        assert_eq!(parsed.items.len(), expect.len(), "case {case}: {text:?}");
        for (it, (name, grams)) in parsed.items.iter().zip(&expect) {
            assert_eq!(it.food.name, *name, "case {case}: {text:?}");
            assert!((it.grams - grams).abs() <= 0.01, "case {case}: {text:?} got {} want {grams}", it.grams);
        }
    }
}
