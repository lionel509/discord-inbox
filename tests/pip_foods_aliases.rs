//! #146 round 5: the short names Pip writes, exact-name matching for reader items, and the reader prompt.
use discord_inbox::foods::{self, SavedFood};
use discord_inbox::pip_model;
use std::path::PathBuf;

fn w() -> SavedFood {
    SavedFood {
        name: "Northwind Pork Wontons".to_string(),
        names: vec!["Northwind Pork Wontons".to_string(), "Pork Wontons".to_string(), "Wontons".to_string()],
        serving_g: Some(140.0),
        serving_pc: Some(8.0),
        kcal: Some(230.0),
        protein: Some(0.0),
        carbs: Some(0.0),
        fat: Some(0.0),
        last: None,
        saved: "2026-10-29".to_string(),
    }
}

#[test]
fn default_aliases_are_the_unbranded_name_and_the_head_noun() {
    let lower = |n: &str| foods::default_aliases(n).iter().map(|a| a.to_lowercase()).collect::<Vec<_>>();
    assert_eq!(lower("Northwind Pork Wontons"), vec!["pork wontons", "wontons"]);
    let d = foods::default_aliases("Northwind Chicken Dumplings");
    assert!(d.iter().all(|a| foods::key_words(a) != vec!["chicken".to_string()]), "{d:?}");
    assert!(!foods::default_aliases("Northwind Salmon").iter().any(|a| a.eq_ignore_ascii_case("salmon")));
    assert!(foods::default_aliases("Wontons").is_empty());
    assert!(foods::default_aliases("Northwind Wontons 24ct").iter().all(|a| !a.chars().any(|c| c.is_ascii_digit())));
}

#[test]
fn only_an_exact_name_counts_as_the_saved_food() {
    let f = w();
    for yes in ["Northwind Pork Wontons", "pork wontons", "Wontons", "steamed wontons"] {
        assert!(foods::names_food(yes, &f), "{yes}");
    }
    for no in ["Wonton soup with broth", "Pork wontons in broth", "wonton soup", "chicken", "pork"] {
        assert!(!foods::names_food(no, &f), "{no}");
    }
}

#[test]
fn the_prompt_names_what_is_already_counted() {
    let photo = vec![PathBuf::from("/tmp/plate.jpg")];
    let counted = vec!["Northwind Pork Wontons (24 pc, label)".to_string()];
    let p = pip_model::prompt_counted("", &photo, &counted);
    assert!(p.contains("Already counted") && p.contains("Northwind Pork Wontons (24 pc, label)"), "{p}");
    assert!(!pip_model::prompt("", &photo).contains("Already counted"));
}

#[test]
fn a_short_name_is_never_only_table_words() {
    assert!(foods::default_aliases("Northwind Chicken Rice").is_empty());
    assert!(foods::default_aliases("Northwind Chicken").is_empty());
    assert!(foods::default_aliases("Northwind Salmon Fillets").is_empty());
    let kw = |n: &str| foods::default_aliases(n).iter().map(|a| foods::key_words(a)).collect::<Vec<_>>();
    assert!(kw("Northwind Beef Noodle Soup").iter().all(|k| *k != vec!["soup".to_string()]));
    assert!(kw("Northwind Mac and Cheese").iter().all(|k| *k != vec!["cheese".to_string()]));
    let p: Vec<String> = foods::default_aliases("Northwind Pork Wontons").iter().map(|a| a.to_lowercase()).collect();
    assert_eq!(p, vec!["pork wontons", "wontons"]);
}

#[test]
fn a_photo_estimate_may_be_the_counted_food_only_by_its_words() {
    let f = w();
    for yes in ["Northwind Wontons", "Wontons", "pork wonton", "steamed wontons"] {
        assert!(foods::may_be_food(yes, &f), "{yes}");
    }
    for no in ["Wonton soup with broth", "Pork wontons in broth", "cooked rice", "", "with"] {
        assert!(!foods::may_be_food(no, &f), "{no}");
    }
}
