//! #146 round 5: a typed piece is a saved food only when its words equal one of the food's names. Made-up fixtures.
use discord_inbox::foods::{self, SavedFood};
use discord_inbox::nutrition::Unit;
use discord_inbox::pip_model::Label;
use std::fs;

fn food(name: &str, names: &[&str], g: f64, pc: Option<f64>, kcal: f64) -> SavedFood {
    SavedFood {
        name: name.to_string(),
        names: names.iter().map(|s| s.to_string()).collect(),
        serving_g: Some(g),
        serving_pc: pc,
        kcal: Some(kcal),
        protein: Some(0.0),
        carbs: Some(0.0),
        fat: Some(0.0),
        last: None,
        saved: "2026-10-29".to_string(),
    }
}

fn all() -> Vec<SavedFood> {
    vec![
        food("Northwind Pork Wontons", &["Northwind Pork Wontons", "Pork Wontons", "Wontons"], 140.0, Some(8.0), 230.0),
        food("Northwind Chicken Dumplings", &["Northwind Chicken Dumplings", "Chicken Dumplings", "Dumplings"], 150.0, Some(10.0), 300.0),
        food("Northwind Rice Bowl", &["Northwind Rice Bowl", "Rice Bowl"], 250.0, None, 400.0),
        food("Northwind Salmon Fillets", &["Northwind Salmon Fillets", "Salmon Fillets", "Fillets"], 113.0, None, 200.0),
    ]
}

const W: &str = "Northwind Pork Wontons";
const D: &str = "Northwind Chicken Dumplings";
const B: &str = "Northwind Rice Bowl";

/// (text, the saved food it names, if any).
const VOCAB: [(&str, Option<&str>); 11] = [
    ("24 wontons", Some(W)),
    ("wontons", Some(W)),
    ("northwind pork wontons 200g", Some(W)),
    ("pork wontons", Some(W)),
    ("dumplings 150g", Some(D)),
    ("northwind rice bowl", Some(B)),
    ("wonton soup", None),
    ("chicken 150g", None),
    ("rice", None),
    ("salmon 150g", None),
    ("cooked rice 180g", None),
];

fn pairs() -> Vec<(String, Vec<&'static str>, Vec<&'static str>)> {
    let mut out = Vec::new();
    for (a, fa) in VOCAB {
        for (b, fb) in VOCAB {
            for j in [", ", " with "] {
                let saved: Vec<&str> = [fa, fb].into_iter().flatten().collect();
                let rest: Vec<&str> = [(a, fa), (b, fb)].into_iter().filter(|(_, f)| f.is_none()).map(|(t, _)| t).collect();
                out.push((format!("{a}{j}{b}"), saved, rest));
            }
        }
    }
    out
}

#[test]
fn a_typed_food_is_never_dropped() {
    let foods = all();
    let cases = pairs();
    assert_eq!(cases.len(), 242);
    for (text, _, want_rest) in cases {
        let t = foods::take(&text, &foods, true);
        assert!(t.unknown.is_empty(), "{text}: {:?}", t.unknown);
        // With nothing matched, `rest` is the text itself, so a " with " joiner is still in it.
        let rest: Vec<&str> = t.rest.split(", ").flat_map(|p| p.split(" with ")).map(str::trim).filter(|s| !s.is_empty()).collect();
        assert_eq!(rest, want_rest, "{text}: rest {:?}", t.rest);
    }
}

#[test]
fn no_food_is_counted_twice() {
    let foods = all();
    for (text, saved, _) in pairs() {
        let t = foods::take(&text, &foods, true);
        let mut got: Vec<&str> = t.items.iter().map(|i| i.food.name).collect();
        let mut want = saved.clone();
        got.sort();
        want.sort();
        assert_eq!(got, want, "{text}");
    }
}

#[test]
fn a_bare_table_food_never_takes_a_multi_word_saved_food() {
    let foods: Vec<SavedFood> = all().into_iter().skip(1).collect();
    for text in ["chicken 150g", "chicken", "rice", "salmon 150g"] {
        let t = foods::take(text, &foods, true);
        assert!(t.items.is_empty(), "{text}");
        assert_eq!(t.rest, text);
    }
}

#[test]
fn a_saved_label_answers_to_wontons_but_not_wonton_soup() {
    let dir = std::env::temp_dir().join(format!("pip-foods-match-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let l = Label { name: W.to_string(), serving_g: Some(140.0), serving_pc: Some(8.0), kcal: Some(230.0), protein: Some(9.0), carbs: Some(28.0), fat: Some(9.0) };
    let f = foods::from_label(&l, "2026-10-29").unwrap();
    foods::upsert(&dir, &f).unwrap();
    let loaded = foods::load(&dir);
    let names: Vec<String> = loaded[0].names.iter().map(|n| n.to_lowercase()).collect();
    assert!(names.contains(&"pork wontons".to_string()) && names.contains(&"wontons".to_string()), "{names:?}");
    let t = foods::take("wontons", &loaded, true);
    assert_eq!(t.items.len(), 1);
    assert_eq!(t.items[0].grams, 140.0);
    let t = foods::take("24 wontons", &loaded, true);
    assert_eq!((t.items[0].grams, t.items[0].qty, t.items[0].unit), (420.0, 24.0, Unit::Piece));
    let t = foods::take("24 wonton", &loaded, true);
    assert_eq!(t.items[0].grams, 420.0);
    let t = foods::take("wonton soup", &loaded, true);
    assert!(t.items.is_empty());
    assert_eq!(t.rest, "wonton soup");
    let t = foods::take("24 wontons, wonton soup", &loaded, true);
    assert_eq!(t.items.len(), 1);
    assert_eq!(t.rest, "wonton soup");
}

#[test]
fn rice_with_a_saved_rice_bowl_is_one_bowl_and_the_rice() {
    let b = vec![all().remove(2)];
    for text in ["rice with northwind rice bowl", "northwind rice bowl with rice"] {
        let t = foods::take(text, &b, true);
        assert_eq!(t.items.len(), 1, "{text}");
        assert_eq!((t.items[0].food.name, t.items[0].grams, t.items[0].unit), (B, 250.0, Unit::Portion), "{text}");
        assert_eq!(t.rest, "rice", "{text}");
    }
}

#[test]
fn resaving_a_label_keeps_the_last_count() {
    let dir = std::env::temp_dir().join(format!("pip-foods-match-last-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let l = Label { name: W.to_string(), serving_g: Some(140.0), serving_pc: Some(8.0), kcal: Some(230.0), protein: Some(9.0), carbs: Some(28.0), fat: Some(9.0) };
    let f = foods::from_label(&l, "2026-10-29").unwrap();
    foods::upsert(&dir, &f).unwrap();
    foods::remember(&dir, W, 24.0, Unit::Piece).unwrap();
    foods::upsert(&dir, &f).unwrap();
    assert_eq!(foods::load(&dir)[0].last, Some((24.0, Unit::Piece)));
}

#[test]
fn a_generic_word_never_takes_a_saved_product_by_its_derived_names() {
    let foods: Vec<SavedFood> = ["Northwind Chicken Rice", "Northwind Beef Noodle Soup", "Northwind Mac and Cheese", "Northwind Chicken"]
        .into_iter()
        .map(|n| {
            let mut names = vec![n.to_string()];
            names.extend(foods::default_aliases(n));
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            food(n, &refs, 200.0, None, 300.0)
        })
        .collect();
    for text in ["rice 150g", "cooked rice 180g", "steamed rice", "soup 300g", "cheese 200g", "chicken 150g"] {
        let t = foods::take(text, &foods, true);
        assert!(t.items.is_empty(), "{text}: {:?}", t.items.iter().map(|i| i.food.name).collect::<Vec<_>>());
        assert!(t.unknown.is_empty(), "{text}");
        assert_eq!(t.rest, text);
    }
}
