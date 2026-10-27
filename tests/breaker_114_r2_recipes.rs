//! Breaker round 2 on PR #114 (recipes): B7 and B8 copied verbatim from the breaker comment
//! 6427879328, and the R2 replacement for B9 (a typed range is reported, not counted). Made-up
//! fixtures only, in the system temp dir.
use discord_inbox::nutrition;
use discord_inbox::recipes::{self, Recipe};
use std::fs;

const SALMON: &str = "---\ntags:\n  - food\n---\n# 🐟 Gochujang Honey Garlic Salmon\n\n## Per 100 g salmon portion\n\n| | g |\n|---|---|\n| Salmon (skinless) | 100 |\n| Soy sauce | 11 |\n| Honey | 12 |\n| Gochujang | 4 |\n| Garlic | 3 |\n| Sesame oil | 2 |\n| Rice vinegar | 3 |\n| Scallion | 9 |\n";

fn salmon_only(name: &str) -> Vec<Recipe> {
    let root = std::env::temp_dir().join(format!("b2p-r2-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("Private").join("Recipes");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gochujang Honey Garlic Salmon.md"), SALMON).unwrap();
    recipes::load(&dir)
}

/// B7: a pound on the recipe path is read as one serving, so "1 lb" scales the salmon to 100 g.
#[test]
fn recipe_typed_pound_scales_to_its_weight() {
    let rs = salmon_only("lb");
    for (text, want) in [("honey garlic salmon 1 lb", 453.6), ("honey garlic salmon 1.5 lb", 680.4)] {
        let h = recipes::resolve(text, &rs).expect("hit");
        let got = h.weight_g.unwrap_or(f64::NAN);
        assert!((got - want).abs() < 0.5, "{text:?}: weight_g {:?}, explicit {} servings {}", h.weight_g, h.explicit, h.servings);
    }
}

/// B8: a different main protein (steak, pork, beef) with a weight matches the salmon recipe at that weight.
#[test]
fn honey_garlic_steak_does_not_match_the_salmon_recipe() {
    let rs = salmon_only("steak");
    for text in ["honey garlic steak 200g", "honey garlic pork 150g", "garlic honey beef 150g"] {
        let hit = recipes::resolve(text, &rs).map(|h| (h.recipe.name.clone(), h.weight_g));
        assert!(hit.is_none(), "{text:?} matched {hit:?}");
    }
}

/// B9 (R2 replacement): a typed range of servings is reported as a range, never counted as one serving.
#[test]
fn typed_serving_range_is_reported_not_counted() {
    let rs = salmon_only("range");
    assert!(recipes::resolve("honey garlic salmon 1-2 servings", &rs).is_none());
    let p = nutrition::parse_meal("honey garlic salmon 1-2 servings");
    assert!(p.unknown.iter().any(|u| u.hint.contains("range")));
}

/// A weight after the full name is a weight, and a bare number after it is a serving count
/// that is too large to be one, so no hit.
#[test]
fn spaced_weight_after_full_name_is_a_weight_not_servings() {
    let rs = salmon_only("spaced");
    let h = recipes::resolve("gochujang honey garlic salmon 150 g", &rs).expect("hit");
    assert!(!h.explicit, "read as {} servings", h.servings);
    assert_eq!(h.weight_g, Some(150.0));
    assert!(recipes::resolve("gochujang honey garlic salmon 150", &rs).is_none());
}
