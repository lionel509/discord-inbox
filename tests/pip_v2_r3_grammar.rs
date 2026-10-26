//! Round-3 rework (U0): one quantity reader for every path (table, exact recipe, fuzzy recipe).
//! Each typed amount is either used as the amount it names or refused. Made-up fixtures only.
use discord_inbox::nutrition::{self, Kind, Unit};
use discord_inbox::recipes::{self, Recipe, Scaled};

const SALMON: &str = "# Gochujang Honey Garlic Salmon\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n";
const BOWL: &str = "# Salmon Rice Bowl\n\n| | g |\n|---|---|\n| Cooked rice | 200 |\n| Salmon | 100 |\n| Soy sauce | 11 |\n";
const TOFU: &str = "# Tofu Stir Fry\n\n| | g |\n|---|---|\n| Broccoli | 200 |\n| Firm tofu | 100 |\n| Soy sauce | 11 |\n";
const FRIED: &str = "# Veggie Fried Rice\n\n| | g |\n|---|---|\n| Cooked rice | 300 |\n| Egg | 50 |\n| Peas | 50 |\n| Oil | 5 |\n";
const EXACT: &str = "gochujang honey garlic salmon";
const FUZZY: &str = "honey garlic salmon";

#[derive(Debug)]
enum Got {
    Grams(f64),
    Ratio(f64),
    Refused(String),
    NoHit,
}

fn note(stem: &str, text: &str) -> Recipe {
    recipes::parse_note(stem, text).expect("fixture has ingredient lines")
}

/// Table path: any unknown is a refusal (hints joined); otherwise the one item's grams.
fn table(input: &str) -> Got {
    let p = nutrition::parse_meal(input);
    if !p.unknown.is_empty() {
        return Got::Refused(p.unknown.iter().map(|u| u.hint.clone()).collect::<Vec<_>>().join("; "));
    }
    assert_eq!(p.items.len(), 1, "{input}: expected one item, got {}", p.items.len());
    Got::Grams(p.items[0].grams)
}

/// Recipe path: a refusal, no hit, or the scale ratio. The rest of a hit must carry no amount.
fn recipe(input: &str, rs: &[Recipe]) -> Got {
    match recipes::resolve_or_refuse(input, rs) {
        Err(r) => Got::Refused(r.unknown.hint),
        Ok(None) => Got::NoHit,
        Ok(Some(h)) => {
            assert!(nutrition::scan(&h.rest).amounts.is_empty(), "{input}: amount left in rest {:?}", h.rest);
            Got::Ratio(recipes::scale(&h, &nutrition::parse_meal(&h.rest)).0)
        }
    }
}

fn close(g: &Got, want: f64) -> bool {
    match g {
        Got::Grams(x) | Got::Ratio(x) => (x - want).abs() <= want * 0.001 + 0.01,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug)]
enum W {
    G(f64),
    R(f64),
    Refuse(&'static str),
}

fn meets(got: &Got, want: W) -> bool {
    match want {
        W::G(x) | W::R(x) => close(got, x),
        W::Refuse(s) => matches!(got, Got::Refused(h) if h.contains(s)),
    }
}

#[test]
fn every_path_uses_or_refuses_each_typed_amount() {
    let rs = vec![note("Gochujang Honey Garlic Salmon", SALMON)];
    // (table input, table want, recipe amount, amount before the name, recipe want)
    let rows: &[(&str, W, &str, bool, W)] = &[
        ("1 1/2 lb salmon", W::G(680.388), "1 1/2 lb", false, W::R(6.80388)),
        ("1 banana (200 g)", W::G(200.0), "(200 g)", false, W::R(2.0)),
        ("two eggs", W::G(100.0), "two", true, W::R(2.0)),
        ("half an avocado", W::G(75.0), "half", true, W::R(0.5)),
        ("salmon x2", W::G(200.0), "x2", false, W::R(2.0)),
        ("salmon 2x", W::G(200.0), "2x", false, W::R(2.0)),
        ("1/2 cup cooked rice", W::G(79.0), "1/2 cup", false, W::Refuse("cup")),
        ("salmon 150", W::Refuse("no unit"), "150", false, W::Refuse("add g")),
        ("2 x 150 g salmon", W::G(300.0), "2 x 150 g", false, W::R(3.0)),
        ("2 lb 4 oz salmon", W::G(1020.583), "2 lb 4 oz", false, W::R(10.20583)),
        ("salmon 1-2 servings", W::Refuse("range"), "1-2 servings", false, W::Refuse("range")),
    ];
    let mut failures = Vec::new();
    for (table_in, table_want, amt, before, recipe_want) in rows {
        let t = table(table_in);
        if !meets(&t, *table_want) {
            failures.push(format!("table {table_in:?}: got {t:?}"));
        }
        for name in [EXACT, FUZZY] {
            let input = if *before { format!("{amt} {name}") } else { format!("{name} {amt}") };
            let got = recipe(&input, &rs);
            if !meets(&got, *recipe_want) {
                failures.push(format!("recipe {input:?}: got {got:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} amounts not used or refused:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn percent_label_is_skipped_on_every_path() {
    let g = table("1 cup 2% milk");
    assert!(close(&g, 244.0), "'1 cup 2% milk' table got {g:?}");
    let rs = vec![note("Gochujang Honey Garlic Salmon", SALMON)];
    for name in [EXACT, FUZZY] {
        let input = format!("{name} 150 g, 1 cup 2% milk");
        match recipes::resolve_or_refuse(&input, &rs) {
            Ok(Some(h)) => {
                let (ratio, _) = recipes::scale(&h, &nutrition::parse_meal(&h.rest));
                assert!((ratio - 1.5).abs() < 0.001, "{input}: ratio {ratio}");
                let p = nutrition::parse_meal(&h.rest);
                assert!(p.unknown.is_empty(), "{input}: rest refused {:?}", p.unknown.iter().map(|u| &u.hint).collect::<Vec<_>>());
                assert_eq!(p.items.len(), 1, "{input}: rest {:?}", h.rest);
                assert!((p.items[0].grams - 244.0).abs() <= 1.0, "{input}: milk {} g", p.items[0].grams);
            }
            Ok(None) => panic!("{input}: no hit"),
            Err(r) => panic!("{input}: refused {}", r.unknown.hint),
        }
    }
}

const FORMS: &[(&str, f64)] = &[
    ("2", 2.0),
    ("3", 3.0),
    ("1/2", 0.5),
    ("½", 0.5),
    ("1 1/2", 1.5),
    ("1½", 1.5),
    ("two", 2.0),
    ("three", 3.0),
    ("half", 0.5),
];
const UNITS: &[&str] = &["g", "oz", "lb", "cup", "tbsp", "pcs", "servings", ""];
const XFORMS: &[&str] = &["x2", "2x", "×2", "x 2", "2 x"];

fn factor(u: &str) -> f64 {
    match u {
        "oz" => 28.35,
        "lb" => 453.592,
        _ => 1.0,
    }
}

/// What a path must give for an amount `v` in unit `u`. None means refused.
fn expect(recipe_path: bool, u: &str, v: f64, before: bool, xform: bool) -> Option<W> {
    let w = match (recipe_path, u) {
        (_, "cup") | (_, "tbsp") => return None,
        (false, "g" | "oz" | "lb") => v * factor(u),
        (false, "pcs") => v * 150.0,
        (false, "servings") => v * 100.0,
        (false, "") if xform => v * 100.0,
        (false, "") if before => v * 150.0,
        (false, "") => return None,
        (false, other) => unreachable!("no table unit {other:?}"),
        (true, "g" | "oz" | "lb") => v * factor(u) / 100.0,
        (true, _) => v,
    };
    Some(if recipe_path { W::R(w) } else { W::G(w) })
}

fn check(path: &str, input: &str, got: &Got, want: Option<W>, failures: &mut Vec<String>) {
    let ok = match want {
        Some(w) => meets(got, w),
        None => matches!(got, Got::Refused(_)),
    };
    if !ok {
        failures.push(format!("{path} {input:?}: wanted {want:?}, got {got:?}"));
    }
}

/// Writer-shaped inputs: every number form, every unit, every position, on every path. One run shows them all.
#[test]
fn generated_amounts_are_used_or_refused_on_every_path() {
    let rs = vec![note("Gochujang Honey Garlic Salmon", SALMON)];
    let mut failures = Vec::new();
    let paths: [(&str, &str, bool); 3] = [("table", "salmon", false), ("exact", EXACT, true), ("fuzzy", FUZZY, true)];
    for (label, food, recipe_path) in paths {
        let run = |input: &str| if recipe_path { recipe(input, &rs) } else { table(input) };
        for (form, v) in FORMS {
            for unit in UNITS {
                let amt = if unit.is_empty() { form.to_string() } else { format!("{form} {unit}") };
                for before in [true, false] {
                    let input = if before { format!("{amt} {food}") } else { format!("{food} {amt}") };
                    let want = expect(recipe_path, unit, *v, before, false);
                    check(label, &input, &run(&input), want, &mut failures);
                }
            }
            // Glued: "2g", "1 1/2lb". Only the digit and vulgar forms, with a mass unit.
            if form.len() <= 5 && !form.chars().all(|c| c.is_ascii_alphabetic()) {
                for unit in ["g", "oz", "lb"] {
                    let amt = format!("{form}{unit}");
                    for before in [true, false] {
                        let input = if before { format!("{amt} {food}") } else { format!("{food} {amt}") };
                        let want = expect(recipe_path, unit, *v, before, false);
                        check(label, &input, &run(&input), want, &mut failures);
                    }
                }
            }
        }
        for xf in XFORMS {
            for before in [true, false] {
                let input = if before { format!("{xf} {food}") } else { format!("{food} {xf}") };
                let want = expect(recipe_path, "", 2.0, before, true);
                check(label, &input, &run(&input), want, &mut failures);
            }
        }
    }
    assert!(failures.is_empty(), "{} generated amounts not used or refused:\n{}", failures.len(), failures.join("\n"));
}

fn kinds(input: &str) -> Vec<(Kind, f64)> {
    nutrition::scan(input).amounts.iter().map(|a| (a.kind, a.value)).collect()
}

fn assert_scan(input: &str, want: &[(Kind, f64)]) {
    let got = kinds(input);
    let same = got.len() == want.len()
        && got.iter().zip(want).all(|(g, w)| g.0 == w.0 && (g.1 - w.1).abs() <= 0.01);
    assert!(same, "{input:?}: got {got:?}, want {want:?}");
}

#[test]
fn scan_reads_each_form_once() {
    assert_scan("1 1/2 lb salmon", &[(Kind::Mass, 680.388)]);
    assert_scan("2 lb 4 oz salmon", &[(Kind::Mass, 1020.583)]);
    assert_scan("1 banana (200 g)", &[(Kind::Bare, 1.0), (Kind::Mass, 200.0)]);
    assert!(nutrition::scan("1 banana (200 g)").amounts[1].bracketed, "the 200 g sits inside brackets");
    let two = nutrition::scan("two eggs");
    assert_eq!(two.amounts.len(), 1);
    assert!(two.amounts[0].word, "'two' is a number word");
    assert_scan("two eggs", &[(Kind::Bare, 2.0)]);
    assert_scan("half an avocado", &[(Kind::Bare, 0.5)]);
    let half = nutrition::scan("half an avocado");
    let spans: Vec<(usize, usize)> = half.amounts.iter().map(|a| (a.start, a.end)).collect();
    assert_eq!(nutrition::strip_spans(&half.text, &spans), "avocado");
    assert_scan("salmon x2", &[(Kind::Multiplier, 2.0)]);
    assert_scan("2x salmon", &[(Kind::Multiplier, 2.0)]);
    assert_scan("x 2 salmon", &[(Kind::Multiplier, 2.0)]);
    assert_scan("2 x 150 g salmon", &[(Kind::Multiplier, 2.0), (Kind::Mass, 150.0)]);
    assert_scan("1 cup 2% milk", &[(Kind::Volume(Unit::Cup), 1.0)]);
    assert_scan("salmon 1-2 servings", &[(Kind::Range, 0.0)]);
    assert_scan("salmon 1 - 2", &[(Kind::Range, 0.0)]);
    let bowl = nutrition::scan("2 servings of salmon bowl");
    assert_scan("2 servings of salmon bowl", &[(Kind::Serving, 2.0)]);
    let spans: Vec<(usize, usize)> = bowl.amounts.iter().map(|a| (a.start, a.end)).collect();
    assert_eq!(nutrition::strip_spans(&bowl.text, &spans), "salmon bowl");
    assert_scan("salmon at 7pm", &[]);
    assert_scan("-100 g salmon", &[(Kind::Negative, 0.0)]);
    assert_scan("one-pan noodles", &[]);
    assert_scan("8 fl oz milk", &[(Kind::Volume(Unit::FlOz), 8.0)]);
}

fn line(input: &str) -> Result<nutrition::Item, String> {
    nutrition::parse_line(input).map_err(|u| u.hint)
}

#[test]
fn bracket_and_multiplier_rules() {
    assert!((line("1 avocado (150 g)").expect("bracketed weight").grams - 150.0).abs() < 0.01);
    assert!((line("2 eggs (about 150 g)").expect("bracketed weight").grams - 150.0).abs() < 0.01);
    let e = line("salmon 150g (200 g)").expect_err("two weights disagree");
    assert!(e.contains("disagree"), "got {e:?}");
    let e = line("1 cup milk (2 cups)").expect_err("two amounts");
    assert!(e.contains("two amounts"), "got {e:?}");
    let s = line("salmon 2 servings").expect("servings");
    assert!((s.grams - 200.0).abs() < 0.01, "2 servings of salmon = {} g", s.grams);
    assert_eq!(s.unit, Unit::Portion);
    assert!(!s.assumed);
    let e = line("salmon x0").expect_err("x0");
    assert!(e.contains("1 to 12"), "got {e:?}");
    assert!((line("salmon 150g x2").expect("150g x2").grams - 300.0).abs() < 0.01);
}

#[test]
fn reference_is_the_title_named_protein() {
    let name_of = |stem: &str, text: &str| {
        let r = note(stem, text);
        recipes::reference(&r).expect("has items").food.name.to_string()
    };
    assert!(name_of("Salmon Rice Bowl", BOWL).contains("salmon"));
    assert!(name_of("Tofu Stir Fry", TOFU).contains("tofu"));
    assert!(name_of("Gochujang Honey Garlic Salmon", SALMON).contains("salmon"));
    assert!(name_of("Veggie Fried Rice", FRIED).contains("rice"));
    let rs = vec![note("Salmon Rice Bowl", BOWL)];
    let h = recipes::resolve("salmon rice bowl 150g", &rs).expect("hit");
    let (ratio, how) = recipes::scale(&h, &nutrition::parse_meal(&h.rest));
    assert!(matches!(how, Scaled::Weight { food, .. } if food.contains("salmon")), "scaled by {how:?}");
    assert!((ratio - 1.5).abs() < 0.001, "ratio {ratio}");
}

#[test]
fn recipe_refusals_carry_the_rest() {
    let rs = vec![note("Gochujang Honey Garlic Salmon", SALMON)];
    let e = recipes::resolve_or_refuse("gochujang honey garlic salmon 150, cooked rice 100g", &rs)
        .err()
        .expect("refused");
    assert!(e.unknown.hint.contains("add g"), "hint {:?}", e.unknown.hint);
    assert_eq!(e.rest, "cooked rice 100g");
    let fried = vec![note("Veggie Fried Rice", FRIED)];
    let e = recipes::resolve_or_refuse("fried rice 1/2 cup", &fried).err().expect("refused");
    assert!(e.unknown.hint.contains("cup"), "hint {:?}", e.unknown.hint);
}
