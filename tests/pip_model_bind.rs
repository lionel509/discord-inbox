//! #130 rework, unit 1 (bind): each weight he types is logged for the food beside it, whatever the reader says.
//! Plan: rework amendment (6460429634) and amendment 2 (6462194933), which wins where they differ.
use discord_inbox::nutrition::{self, Parsed, Unknown};
use discord_inbox::pip_model;
use std::fs;

const UNBOUND: &str = "you typed this weight but no food the reader listed matches it; not counted";
const TIED: &str = "this typed weight fits more than one food; those lines are estimates";
const EACH: &str = "a weight typed with \"each\" is not split across foods; type a weight beside each food";
const TYPED_RANGE: &str = "you typed this weight; one food must be 1 g to 5 kg";
const MORE: &str = "not counted";

fn it(name: &str, grams: f64, source: &str, kcal: f64) -> String {
    format!(r#"{{"name":"{name}","grams":{grams},"source":"{source}","kcal_100":{kcal},"protein_100":4.5,"carbs_100":25,"fat_100":2.1,"veg":false}}"#)
}

fn reply(items: &[String]) -> String {
    format!(r#"{{"items":[{}],"unreadable":[]}}"#, items.join(","))
}

fn run(text: &str, items: &[String]) -> Parsed {
    pip_model::parse(text, &reply(items)).expect("object with items")
}

fn span(grams: f64, words: &[&str]) -> nutrition::TypedSpan {
    nutrition::TypedSpan { grams, words: words.iter().map(|w| w.to_string()).collect() }
}

fn unknown(text: &str, hint: &str) -> Unknown {
    Unknown { text: text.to_string(), hint: hint.to_string() }
}

/// The logged items whose food name contains `needle`.
fn named<'a>(p: &'a Parsed, needle: &str) -> Vec<&'a nutrition::Item> {
    p.items.iter().filter(|i| i.food.name.contains(needle)).collect()
}

#[test]
fn typed_spans_read_his_dm() {
    assert_eq!(
        nutrition::typed_spans("100 grnas of noodle 100 grams of chicken"),
        vec![span(100.0, &["noodle"]), span(100.0, &["chicken"])]
    );
}

#[test]
fn typed_spans_shapes() {
    assert_eq!(nutrition::typed_spans("chicken 100g, rice 200g"), vec![span(100.0, &["chicken"]), span(200.0, &["rice"])]);
    assert_eq!(nutrition::typed_spans("200g chicken and rice"), vec![span(200.0, &["chicken"])]);
    assert!(nutrition::typed_spans("2 eggs and a glass of water").is_empty());
    assert!(nutrition::typed_spans("100 grapes").is_empty());
    assert!(nutrition::typed_spans("100 grains of rice").is_empty());
    assert!(nutrition::typed_spans(" 300g").is_empty());
    let oz = nutrition::typed_spans("8 oz steak");
    assert_eq!(oz.len(), 1, "{oz:?}");
    assert_eq!(oz[0].words, vec!["steak".to_string()]);
    assert!((oz[0].grams - nutrition::typed_grams("8 oz")[0]).abs() < 0.1);
}

#[test]
fn swapped_weights_follow_their_food() {
    let p = run("100g chicken and 200g rice", &[it("chicken", 200.0, "text", 165.0), it("cooked rice", 100.0, "text", 130.0)]);
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
    let chicken = named(&p, "chicken");
    let rice = named(&p, "rice");
    assert_eq!(chicken.len(), 1);
    assert_eq!(rice.len(), 1);
    assert!(!chicken[0].estimated && chicken[0].grams == 100.0, "chicken: {}", chicken[0].grams);
    assert!(!rice[0].estimated && rice[0].grams == 200.0, "rice: {}", rice[0].grams);
}

#[test]
fn typo_in_his_food_word_binds() {
    let p = run("150 grams chiken", &[it("chicken", 120.0, "estimate", 165.0)]);
    let c = named(&p, "chicken");
    assert_eq!(c.len(), 1, "{:?}", p.unknown);
    assert!(!c[0].estimated && c[0].grams == 150.0);
}

#[test]
fn rice_does_not_inherit_the_weight() {
    let p = run("200g chicken and rice", &[it("chicken", 200.0, "text", 165.0), it("cooked rice", 150.0, "text", 130.0)]);
    let c = named(&p, "chicken");
    let r = named(&p, "rice");
    assert!(!c[0].estimated && c[0].grams == 200.0);
    assert!(r[0].estimated && r[0].grams == 150.0, "rice keeps the reader's own estimate");
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn count_units_never_bind() {
    let p = run("2 eggs", &[it("egg", 100.0, "text", 143.0)]);
    let e = named(&p, "egg");
    assert_eq!(e.len(), 1);
    assert!(e[0].estimated && e[0].grams == 100.0);
    assert!(p.unknown.is_empty());
}

#[test]
fn oz_binds_exactly() {
    let p = run("8 oz steak", &[it("steak", 230.0, "text", 200.0)]);
    let want = nutrition::typed_spans("8 oz steak")[0].grams;
    let s = named(&p, "steak");
    assert_eq!(s.len(), 1, "{:?}", p.unknown);
    assert!(!s[0].estimated && (s[0].grams - want).abs() < 1e-9);
}

#[test]
fn scale_label_on_typed_weight_is_typed() {
    let p = run("100g chicken breast", &[it("chicken breast", 100.0, "scale", 120.0)]);
    let c = named(&p, "chicken breast");
    assert_eq!(c.len(), 1);
    assert!(!c[0].estimated && !c[0].scale && c[0].grams == 100.0);
}

#[test]
fn unmatched_typed_weight_is_reported() {
    let p = run("100 grams of chicken", &[it("cabbage", 100.0, "text", 25.0)]);
    assert_eq!(p.items.len(), 1);
    assert!(p.items[0].estimated, "cabbage is not the typed food, so its line is an estimate");
    assert_eq!(p.unknown, vec![unknown("100 g chicken", UNBOUND)]);
}

#[test]
fn tied_span_binds_nobody() {
    let p = run("200g chicken rice", &[it("chicken", 100.0, "text", 165.0), it("cooked rice", 100.0, "text", 130.0)]);
    assert_eq!(p.items.len(), 2);
    assert!(p.items.iter().all(|i| i.estimated), "both lines are estimates");
    assert_eq!(p.unknown, vec![unknown("200 g chicken rice", TIED)]);
}

#[test]
fn water_is_a_zero_kcal_item() {
    for (text, name) in [("water", "water"), ("glass of water", "water"), ("sparkling water", "sparkling water")] {
        let p = run(text, &[it(name, 250.0, "text", 0.0)]);
        assert_eq!(p.items.len(), 1, "{text}: {:?}", p.unknown);
        assert_eq!(p.items[0].food.name, name, "{text}");
        assert_eq!(nutrition::macros(&p.items).kcal, 0.0, "{text}");
        assert!(p.items[0].estimated, "{text}: no weight typed beside it");
        assert!(p.unknown.is_empty(), "{text}: {:?}", p.unknown);
    }
    assert!(nutrition::table_food("water").is_none(), "water is not a table row (it would break lookup)");
    assert_eq!(nutrition::lookup("salmon poached in water").expect("salmon").name, "salmon");
    let p = run("cooked egg noodles", &[it("cooked egg noodles", 100.0, "text", 0.0)]);
    assert!(p.items.is_empty(), "a non-water food with no calories stays unknown");
    assert_eq!(p.unknown.first().map(|u| u.hint.as_str()), Some("the reader gave no calories"));
}

#[test]
fn is_image_by_magic_or_extension() {
    let dir = std::env::temp_dir().join(format!("pip-bind-{}-image", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let png = dir.join("x.bin");
    fs::write(&png, [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]).unwrap();
    let jpg = dir.join("x.jpg");
    fs::write(&jpg, b"jpeg bytes").unwrap();
    let jpeg_magic = dir.join("y.bin");
    fs::write(&jpeg_magic, [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
    let txt = dir.join("notes.txt");
    fs::write(&txt, b"file bytes").unwrap();
    assert!(pip_model::is_image(&png), "PNG magic");
    assert!(pip_model::is_image(&jpg), "a .jpg name");
    assert!(pip_model::is_image(&jpeg_magic), "JPEG magic");
    assert!(!pip_model::is_image(&txt), "a text file");
    assert!(!pip_model::is_image(&dir.join("missing.txt")), "a missing file");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn echo_is_skipped_but_a_lone_example_is_kept() {
    let answer = r#"{"items":[{"name":"grilled tilapia","grams":180,"source":"estimate","kcal_100":128,"protein_100":20,"carbs_100":0,"fat_100":3,"veg":false}],"unreadable":[]}"#;
    let example = r#"{"items":[{"name":"cooked egg noodles","grams":100,"source":"text","kcal_100":140,"protein_100":4.5,"carbs_100":25,"fat_100":2.1,"veg":false}],"unreadable":[]}"#;
    let p = pip_model::parse("", &format!("{answer}\n{example}")).expect("parses");
    assert_eq!(p.items.len(), 1, "{:?}", p.unknown);
    assert!(p.items[0].food.name.contains("tilapia"), "the answer wins over the echo");
    assert_eq!(p.items[0].grams, 180.0);
    let p = pip_model::parse("", example).expect("parses");
    assert_eq!(p.items.len(), 1, "{:?}", p.unknown);
    assert!(p.items[0].food.name.contains("noodles"), "a lone example is still the answer");
    assert_eq!(p.items[0].grams, 100.0);
}

#[test]
fn same_food_twice_binds_in_typed_order() {
    let p = run("100g chicken and 200g chicken", &[it("chicken", 150.0, "text", 165.0), it("chicken", 150.0, "text", 165.0)]);
    let grams: Vec<f64> = p.items.iter().map(|i| i.grams).collect();
    assert_eq!(grams, vec![100.0, 200.0], "{:?}", p.unknown);
    assert!(p.items.iter().all(|i| !i.estimated && !i.scale));
    assert!(p.unknown.is_empty());

    let p = run("100g chicken and 200g chicken", &[it("chicken", 150.0, "text", 165.0), it("chicken breast", 150.0, "text", 165.0)]);
    let chicken = p.items.iter().find(|i| i.food.name == "chicken").expect("chicken");
    let breast = p.items.iter().find(|i| i.food.name == "chicken breast").expect("chicken breast");
    assert_eq!(chicken.grams, 100.0);
    assert_eq!(breast.grams, 200.0);
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn glass_is_a_count_not_grams() {
    assert!(nutrition::typed_spans("2 glass of water").is_empty());
    assert!(nutrition::typed_spans("2 glass of wine").is_empty());
    assert_eq!(nutrition::typed_spans("2 g salt"), vec![span(2.0, &["salt"])]);
    assert_eq!(nutrition::typed_spans("100 grnas of noodle"), vec![span(100.0, &["noodle"])]);
    assert_eq!(nutrition::typed_spans("150 gms rice"), vec![span(150.0, &["rice"])]);
    let p = run("2 glass of wine", &[it("wine", 300.0, "estimate", 85.0)]);
    assert_eq!(p.items.len(), 1, "{:?}", p.unknown);
    assert!(p.items[0].estimated && p.items[0].grams == 300.0);
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn short_food_word_is_listed_not_dropped() {
    assert_eq!(nutrition::typed_spans("100g pb"), vec![span(100.0, &["pb"])]);
    assert_eq!(nutrition::typed_spans("밥 200g"), vec![span(200.0, &["밥"])]);
    assert!(nutrition::typed_spans("100g").is_empty(), "a weight with no food beside it makes no span");
    let p = run("100g pb", &[it("peanut butter", 32.0, "estimate", 588.0)]);
    assert_eq!(p.unknown, vec![unknown("100 g pb", UNBOUND)]);
    let pb = named(&p, "peanut");
    assert_eq!(pb.len(), 1);
    assert!(pb[0].estimated && pb[0].grams == 32.0);
}

#[test]
fn each_is_listed_not_split() {
    assert_eq!(nutrition::typed_spans("chicken and rice, 100g each"), vec![span(100.0, &["each"])]);
    assert_eq!(nutrition::typed_spans("100g each of chicken and rice"), vec![span(100.0, &["each", "chicken"])]);
    let p = run("chicken and rice, 100g each", &[it("chicken", 150.0, "text", 165.0), it("cooked rice", 150.0, "text", 130.0)]);
    assert_eq!(p.items.len(), 2, "{:?}", p.unknown);
    assert!(p.items.iter().all(|i| i.estimated && i.grams == 150.0), "the foods stay estimates");
    assert_eq!(p.unknown, vec![unknown("100 g each", EACH)]);
}

#[test]
fn count_after_weight_ends_the_span() {
    assert_eq!(nutrition::typed_spans("200g chicken 2 eggs"), vec![span(200.0, &["chicken"])]);
    assert_eq!(nutrition::typed_spans("chicken 200g 2 eggs"), vec![span(200.0, &["chicken"])]);
    let p = run("200g chicken 2 eggs", &[it("chicken", 200.0, "text", 165.0), it("egg", 100.0, "estimate", 155.0)]);
    let c = named(&p, "chicken");
    let e = named(&p, "egg");
    assert!(!c[0].estimated && c[0].grams == 200.0);
    assert!(e[0].estimated && e[0].grams == 100.0);
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn near_echo_after_answer_is_skipped() {
    // The echo test is on the example's food and grams, not on the whole object (review item 9).
    let answer = r#"{"items":[{"name":"grilled tilapia","grams":180,"source":"estimate","kcal_100":128,"protein_100":20,"carbs_100":0,"fat_100":3,"veg":false}],"unreadable":[]}"#;
    let echo = r#"{"items":[{"name":"cooked egg noodles","grams":100,"source":"text","kcal_100":140,"protein_100":4.5,"carbs_100":25,"fat_100":2.1,"veg":false}],"unreadable":[]}"#;
    let p = pip_model::parse("", &format!("{answer}\n{echo}")).expect("parses");
    assert_eq!(p.items.len(), 1, "{:?}", p.unknown);
    assert!(p.items[0].food.name.contains("tilapia"));
    assert_eq!(p.items[0].grams, 180.0);
}

#[test]
fn typed_weight_out_of_range_says_typed() {
    let p = run("6 kg chicken", &[it("chicken breast", 600.0, "text", 165.0)]);
    assert!(p.items.is_empty(), "{:?}", p.items);
    assert_eq!(p.unknown, vec![unknown("chicken breast 6000 g", TYPED_RANGE)]);
}

#[test]
fn plurals_bind_berries_and_tomatoes() {
    let p = run("100g berries", &[it("berry", 80.0, "text", 57.0)]);
    let b = named(&p, "berry");
    assert_eq!(b.len(), 1, "{:?}", p.unknown);
    assert!(!b[0].estimated && b[0].grams == 100.0);
    assert!(p.unknown.is_empty());

    let p = run("200g tomatoes", &[it("tomato", 150.0, "text", 18.0)]);
    let t = named(&p, "tomato");
    assert_eq!(t.len(), 1, "{:?}", p.unknown);
    assert!(!t[0].estimated && t[0].grams == 200.0);
}

#[test]
fn not_counted_is_capped_at_ten() {
    let text = "10g alpha, 20g bravo, 30g charlie, 40g delta, 50g echo, 60g foxtrot, 70g golf, 80g hotel, 90g india, 100g juliet, 110g kiwi, 120g lima";
    let p = run(text, &[it("onion", 60.0, "estimate", 40.0)]);
    assert_eq!(p.items.len(), 1);
    assert!(p.items[0].estimated && p.items[0].grams == 60.0);
    let words = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet"];
    let mut want: Vec<Unknown> = words.iter().enumerate().map(|(i, w)| unknown(&format!("{} g {w}", (i + 1) * 10), UNBOUND)).collect();
    want.push(unknown("and 2 more typed weights", MORE));
    assert_eq!(p.unknown, want);
}

#[test]
fn noodle_dm_binds_typed_and_keeps_scale() {
    let p = run(
        "100 grnas of noodle 100 grams of chicken",
        &[
            it("cooked egg noodles", 150.0, "text", 138.0),
            it("chicken breast", 120.0, "text", 165.0),
            it("cabbage", 87.0, "scale", 25.0),
            it("onion", 60.0, "estimate", 40.0),
        ],
    );
    let noodle = named(&p, "noodle");
    assert_eq!(noodle.len(), 1, "{:?}", p.unknown);
    assert!(noodle[0].grams == 100.0 && !noodle[0].estimated && !noodle[0].scale);
    let chicken = named(&p, "chicken");
    assert_eq!(chicken.len(), 1, "{:?}", p.unknown);
    assert!(chicken[0].grams == 100.0 && !chicken[0].estimated && !chicken[0].scale);
    assert!(p.items.iter().all(|i| i.grams != 120.0 && i.grams != 150.0), "the reader's numbers for typed foods never show");
    let cabbage = named(&p, "cabbage");
    assert!(cabbage[0].scale && !cabbage[0].estimated && cabbage[0].grams == 87.0);
    let onion = named(&p, "onion");
    assert!(onion[0].estimated && onion[0].grams == 60.0);
    assert!(p.unknown.is_empty(), "{:?}", p.unknown);
}

#[test]
fn property_each_typed_weight_binds_to_its_food() {
    // (his word, his typo, the reader's name for it)
    let foods = [
        ("chicken", "chiken", "chicken breast"),
        ("noodle", "noodle", "cooked egg noodles"),
        ("broccoli", "brocoli", "broccoli"),
        ("salmon", "salmn", "grilled salmon"),
        ("tofu", "tofu", "firm tofu"),
        ("cabbage", "cabage", "cabbage"),
        ("rice", "rice", "cooked white rice"),
    ];
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        s >> 33
    };
    let amount_joins = [" ", " and ", ", ", "\n"];
    let food_joins = [", ", " and ", "\n"];
    for case in 0..400 {
        let n = 1 + (next() % 4) as usize;
        let mut picked: Vec<usize> = Vec::new();
        while picked.len() < n {
            let f = (next() % foods.len() as u64) as usize;
            if !picked.contains(&f) {
                picked.push(f);
            }
        }
        // One case in four types one of its foods twice, with two different weights in the same word form.
        let twice = if case % 4 == 0 { Some(picked[0]) } else { None };
        let amount_first = next() % 2 == 0;
        // (food index, typed grams, the text of that weight)
        let mut forms: Vec<(usize, f64, String)> = Vec::new();
        for &f in &picked {
            let (word, typo, _) = foods[f];
            let food = if next() % 2 == 0 { word } else { typo };
            let copies = if twice == Some(f) { 2 } else { 1 };
            let mut used: Vec<u64> = Vec::new();
            for _ in 0..copies {
                let mut w = 5 + next() % 995;
                while used.contains(&w) {
                    w = 5 + next() % 995;
                }
                used.push(w);
                let form = if amount_first {
                    let options = [format!("{w}g {food}"), format!("{w} grams of {food}"), format!("{w} grnas of {food}"), format!("{w} gms {food}")];
                    options[(next() % 4) as usize].clone()
                } else {
                    format!("{food} {w}g")
                };
                forms.push((f, w as f64, form));
            }
        }
        let joins: &[&str] = if amount_first { &amount_joins } else { &food_joins };
        let mut text = String::new();
        for (k, (_, _, form)) in forms.iter().enumerate() {
            if k > 0 {
                text.push_str(joins[(next() % joins.len() as u64) as usize]);
            }
            text.push_str(form);
        }
        // The reader lists each typed food once per weight, under its own name, with its own grams or a wrong one.
        let mut items: Vec<String> = Vec::new();
        for (f, w, _) in &forms {
            let grams = if next() % 2 == 0 { *w } else { (1 + next() % 2999) as f64 };
            let source = ["text", "scale", "estimate"][(next() % 3) as usize];
            items.push(it(foods[*f].2, grams, source, 150.0));
        }
        let onion = next() % 2 == 0;
        if onion {
            let source = ["text", "scale", "estimate"][(next() % 3) as usize];
            items.push(it("onion", 60.0, source, 40.0));
        }
        let p = pip_model::parse(&text, &reply(&items)).unwrap_or_else(|| panic!("case {case}: no parse"));
        assert!(p.unknown.is_empty(), "case {case}: {text}: {:?}", p.unknown);
        assert_eq!(p.items.len(), forms.len() + usize::from(onion), "case {case}: {text}");
        for &f in &picked {
            let reader = foods[f].2;
            let name = nutrition::table_food(reader).map_or(reader, |food| food.name);
            let mut got: Vec<f64> = p.items.iter().filter(|i| i.food.name == name).map(|i| i.grams).collect();
            let mut want: Vec<f64> = forms.iter().filter(|(g, _, _)| *g == f).map(|(_, w, _)| *w).collect();
            got.sort_by(f64::total_cmp);
            want.sort_by(f64::total_cmp);
            assert_eq!(got, want, "case {case}: {text}");
            for i in p.items.iter().filter(|i| i.food.name == name) {
                assert!(!i.estimated && !i.scale, "case {case}: typed {name} is not marked: {text}");
            }
        }
        if onion {
            let o: Vec<_> = p.items.iter().filter(|i| i.food.name == "onion").collect();
            assert_eq!(o.len(), 1, "case {case}: {text}");
            assert!(o[0].estimated || o[0].scale, "case {case}: onion has no typed weight");
        }
    }
}

// #130 rework round 4: fix items 1-3 (breaker u1r/u2r round 3, review 5679195866).

#[test]
fn card_in_model_name_with_price_never_survives() {
    let got = pip_model::enforce("100g chicken", "4111.1111.1111.1111 12.99", 100.0, pip_model::Source::Text, 165.0, 1.0, 1.0, 1.0, false);
    let leaked = match &got {
        Ok(item) => item.food.name.contains("4111"),
        Err(u) => u.text.contains("4111"),
    };
    assert!(!leaked, "a card survived in the model name");
}

#[test]
fn breaker_r3_trailing_weight_does_not_drop_leading_span() {
    let p = run("100g chicken 50g", &[it("chicken", 150.0, "estimate", 165.0)]);
    let c = named(&p, "chicken");
    assert!(c.len() == 1 && (c[0].grams - 100.0).abs() < 0.1 && !c[0].estimated, "chicken rows {:?}", p.items.iter().map(|i| (i.grams, i.estimated)).collect::<Vec<_>>());
}

#[test]
fn trailing_bare_weight_makes_no_span() {
    assert_eq!(nutrition::typed_spans("100g chicken 50g"), vec![span(100.0, &["chicken"])]);
}

#[test]
fn breaker_r3_bound_item_failing_no_calories_keeps_typed_weight_listed() {
    let p = run("100g chicken", &[it("chicken", 150.0, "text", 0.0)]);
    assert!(p.items.is_empty(), "items {}", p.items.len());
    let texts: Vec<String> = p.unknown.iter().map(|u| u.text.clone()).collect();
    assert!(texts.iter().any(|t| t.contains("100 g chicken")), "typed 100 g chicken vanished; unknown = {texts:?}");
}

#[test]
fn bound_item_with_no_reader_grams_logs_the_typed_weight() {
    let reply_text = r#"{"items":[{"name":"chicken","kcal_100":165,"source":"text"}],"unreadable":[]}"#;
    let p = pip_model::parse("100g chicken", reply_text).expect("object with items");
    assert_eq!(p.items.len(), 1, "items {}", p.items.len());
    assert!(p.items[0].grams == 100.0, "grams {}", p.items[0].grams);
    assert!(!p.items[0].estimated);
    assert!(p.unknown.is_empty(), "unknown = {:?}", p.unknown);
}

#[test]
fn bound_item_with_a_long_name_names_the_typed_weight() {
    let long = format!("chicken {}", "x".repeat(70));
    let p = run("100g chicken", &[it(&long, 150.0, "text", 165.0)]);
    assert!(p.items.is_empty(), "items {}", p.items.len());
    let texts: Vec<String> = p.unknown.iter().map(|u| u.text.clone()).collect();
    assert!(texts.iter().any(|t| t.starts_with("100 g ")), "typed weight not first; unknown = {texts:?}");
}

#[test]
fn rejected_name_containing_typed_number_still_names_weight() {
    let p = run("50g pasta", &[it("pasta 150 g", 150.0, "text", 0.0)]);
    assert!(p.items.is_empty(), "items {}", p.items.len());
    assert_eq!(p.unknown, vec![unknown("50 g pasta 150 g", "the reader gave no calories")]);
}

#[test]
fn typed_weight_cap_is_5_kg() {
    let p = run("5000g chicken", &[it("chicken breast", 600.0, "text", 165.0)]);
    assert_eq!(p.items.len(), 1, "items {}", p.items.len());
    assert!(p.items[0].grams == 5000.0 && !p.items[0].estimated, "grams {}", p.items[0].grams);
    let p = run("5001g chicken", &[it("chicken breast", 600.0, "text", 165.0)]);
    assert!(p.items.is_empty(), "items {}", p.items.len());
    assert_eq!(p.unknown, vec![unknown("chicken breast 5001 g", TYPED_RANGE)]);
}

#[test]
fn unbound_item_without_grams_says_no_weight() {
    let reply_text = r#"{"items":[{"name":"rice","source":"estimate","kcal_100":130}],"unreadable":[]}"#;
    let p = pip_model::parse("rice for lunch", reply_text).expect("object with items");
    assert!(p.items.is_empty(), "items {}", p.items.len());
    assert_eq!(p.unknown, vec![unknown("rice", "the reader gave no weight")]);
}

#[test]
fn nameless_item_without_grams_says_no_usable_name() {
    let reply_text = r#"{"items":[{"source":"estimate","kcal_100":130}],"unreadable":[]}"#;
    let p = pip_model::parse("rice for lunch", reply_text).expect("object with items");
    assert!(p.items.is_empty(), "items {}", p.items.len());
    assert_eq!(p.unknown, vec![unknown("(unnamed item)", "the reader gave no usable name")]);
}
