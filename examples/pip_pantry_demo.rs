//! #159 demo: a bought-products pantry and one honey-ham meal DM with a sandwich photo, through the real meal reader.
//! Run with `scripts/demo-159.sh <photo>`. Scratch folders only; the photo is copied in and never written to the repo.
use discord_inbox::nutrition;
use discord_inbox::pantry::{self, Product};
use discord_inbox::pip;
use discord_inbox::pip_model::{self, Reading, Sources};
use std::fs;
use std::path::Path;
use std::time::Duration;

const TEXT: &str = "3 honey ham\n2egg\nOne chese\nObe table spoon hummas";
const TODAY: &str = "2026-11-10";
const MAIN_KCAL: i64 = 531;

fn product(name: &str, brand: &str, size: &str, bought: &str) -> Product {
    Product { name: name.into(), brand: brand.into(), size: size.into(), bought: bought.into() }
}

fn main() {
    let Some(photo) = std::env::args().nth(1) else {
        eprintln!("usage: pip_pantry_demo <photo>");
        std::process::exit(2);
    };
    let root = std::env::temp_dir().join(format!("pip-159-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let note = root.join("Kitchen/Pantry/On Hand.md");
    fs::create_dir_all(note.parent().unwrap()).unwrap();
    let seed = [
        product("Farmhouse Sourdough Bread", "Pepperidge Farm", "24 oz", "2026-11-08"),
        product("Uncured Black Forest Ham Ultra-Thin", "Good & Gather", "16 oz", "2026-11-08"),
        product("Mild Cheddar Cheese Slices", "Good & Gather", "8 oz / 12 slices", "2026-11-08"),
        product("Roasted Garlic Hummus", "Good & Gather", "10 oz", "2026-11-08"),
        product("Chicken Broth", "Swanson", "", "2026-11-08"),
        product("Bananas", "", "", "2026-11-08"),
        product("Organic Eggs", "Costco", "", "2026-10-31"),
    ];
    fs::write(&note, pantry::render_with(&[], &seed, TODAY)).unwrap();
    let products = pantry::products_at(&note, TODAY);
    println!("INFO pantry offers {} products", products.len());

    let photo_dst = root.join("sandwich.jpg");
    fs::copy(Path::new(&photo), &photo_dst).expect("the photo copies into the scratch folder");
    let cmd = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/pip-model.sh");
    let src = Sources { foods: Vec::new(), dishes: Vec::new(), products };
    let parsed = match pip_model::read_sources(&cmd, TEXT, &[photo_dst], &src, Duration::from_secs(90)) {
        Reading::Read(p) => p,
        _ => {
            println!("FAIL the reader gave no reading");
            std::process::exit(1);
        }
    };

    let bought: Vec<String> = parsed.items.iter().filter_map(|i| i.food.source.strip_prefix("bought: ").map(str::to_string)).collect();
    let mut assumed = Vec::new();
    if !bought.is_empty() {
        assumed.push(format!("From what you bought: {}.", bought.join(", ")));
    }
    println!("{}", pip::reply_text(&parsed, None, &[], &[], &assumed, None));
    for i in &parsed.items {
        println!("LINE {}", nutrition::item_line(i));
    }

    let mut failed = false;
    for who in ["Farmhouse Sourdough Bread", "Uncured Black Forest Ham Ultra-Thin", "Mild Cheddar Cheese Slices", "Roasted Garlic Hummus"] {
        let ok = parsed.items.iter().any(|i| i.food.source == format!("bought: {who}"));
        println!("{} a line with bought: {who}", if ok { "PASS" } else { "FAIL" });
        failed |= !ok;
    }
    let bread = format!("bought: {}", "Farmhouse Sourdough Bread");
    let slices_ok = parsed.items.iter().filter(|i| i.food.source == bread).any(|i| {
        let k = (i.grams / 43.0).round();
        (1.0..=4.0).contains(&k) && (i.grams - k * 43.0).abs() <= 4.0
    });
    println!("{} the sourdough grams are 43 g per slice for 1 to 4 slices", if slices_ok { "PASS" } else { "FAIL" });
    failed |= !slices_ok;

    let kcal = nutrition::macros(&parsed.items).kcal.round() as i64;
    println!("INFO total {kcal} kcal (main gave {MAIN_KCAL}; about 560-590 expected)");
    let _ = fs::remove_dir_all(&root);
    if failed {
        std::process::exit(1);
    }
}
