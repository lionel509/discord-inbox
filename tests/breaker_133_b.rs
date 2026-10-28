//! Breaker tests for PR #133 unit B (src/nutrition.rs and pip_model::enforce: estimated/scale flags,
//! typed_grams). Only the tests that FAIL against head b1ed25e are kept. Each encodes a promise from
//! issue #130 and its plan amendment: typed weights are authoritative, a weight with no typed or photo
//! source is marked estimated, and a scale tag means measured.
use discord_inbox::nutrition::{self, Item};
use discord_inbox::pip_model::{enforce, Source};

const NO_VEG: bool = false;

fn show(r: &Result<Item, nutrition::Unknown>) -> String {
    match r {
        Ok(i) => format!("Ok(grams={}, estimated={}, scale={}, line={:?})", i.grams, i.estimated, i.scale, nutrition::item_line(i)),
        Err(u) => format!("Err({:?}, {:?})", u.text, u.hint),
    }
}

// B1. Typed weight overruled by the reader (Text source). He typed 100 g; the reader says 300 g.
// Expected: logged at 100 g, or refused. Actual: 300 g logged (flagged estimated, kcal from 300 g).
#[test]
fn typed_weight_not_overruled_by_text_source() {
    let r = enforce("100 g noodle", "noodle", 300.0, Source::Text, 138.0, 4.5, 25.0, 2.1, NO_VEG);
    let ok_typed = matches!(&r, Ok(i) if (i.grams - 100.0).abs() < 0.6);
    assert!(ok_typed || r.is_err(), "typed 100 g logged as {}", show(&r));
}

// B2. Typed weight overruled by an estimate. Expected: 100 g or refused. Actual: 300 g estimated.
#[test]
fn typed_weight_not_overruled_by_estimate() {
    let r = enforce("100 g chicken", "chicken", 300.0, Source::Estimate, 165.0, 31.0, 0.0, 3.6, NO_VEG);
    let ok_typed = matches!(&r, Ok(i) if (i.grams - 100.0).abs() < 0.6);
    assert!(ok_typed || r.is_err(), "typed 100 g chicken logged as {}", show(&r));
}

// B3. Typed weight overruled by a scale tag, which is also not marked estimated. Expected: 100 g or refused.
// Actual: 300 g with "(scale)" and no estimate flag, so the typed number is silently replaced by a "measured" one.
#[test]
fn typed_weight_not_overruled_by_scale() {
    let r = enforce("100 g pasta", "pasta", 300.0, Source::Scale, 131.0, 5.0, 25.0, 1.1, NO_VEG);
    let ok_typed = matches!(&r, Ok(i) if (i.grams - 100.0).abs() < 0.6);
    assert!(ok_typed || r.is_err(), "typed 100 g pasta with a scale tag logged as {}", show(&r));
}

// B5. Two foods, two typed weights. "100g chicken and 200g rice": the reader gives chicken 200 g. The 200 g
// is typed, but for rice, so the chicken line must be flagged. Actual: 200 g chicken, unflagged.
#[test]
fn swapped_weight_between_two_foods_is_flagged() {
    let r = enforce("100g chicken and 200g rice", "chicken", 200.0, Source::Text, 165.0, 31.0, 0.0, 3.6, NO_VEG);
    let ok = matches!(&r, Ok(i) if i.estimated || (i.grams - 100.0).abs() < 0.6);
    assert!(ok || r.is_err(), "chicken 200 g (he typed 100 g chicken) logged as {}", show(&r));
}
