//! #159 U1: bought products in the kitchen pantry note, filled by `restock` from receipt OCR (or a store-app
//! purchase-history screen) through one extractor call. Made-up folders and fixtures; no live model.
use discord_inbox::pantry::{self, Command, Product, Row};
use discord_inbox::vaults;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const TODAY: &str = "2026-11-10";

fn root(name: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("di-pantry-products-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(Path::new(vaults::PANTRY).parent().unwrap())).unwrap();
    fs::create_dir_all(dir.join(vaults::SPLITS)).unwrap();
    dir
}

fn pantry_file(root: &Path) -> PathBuf {
    root.join(vaults::PANTRY)
}

fn read(root: &Path) -> String {
    fs::read_to_string(pantry_file(root)).unwrap_or_default()
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// A receipt note in the folder the solo-receipt branch reads (the first segment of `vaults::SPLITS`).
fn receipt(root: &Path, date: &str, title: &str, ocr: &str) {
    let folder = vaults::SPLITS.split('/').next().unwrap();
    put(root, &format!("{folder}/{date} {title}.md"), &format!("## OCR 1.jpg\n\n~~~text\n{ocr}\n~~~\n"));
}

fn split_note(who_rows: &[(&str, &str)]) -> String {
    let mut s = String::from("# Costco Split — 2026-10-02\n\n| Item | Qty | Who | Price |\n|---|---|---|---|\n");
    for (item, who) in who_rows {
        s.push_str(&format!("| {item} | 1 | {who} | 1.00 |\n"));
    }
    s
}

fn prod(name: &str, brand: &str, size: &str, bought: &str) -> Product {
    Product { name: name.into(), brand: brand.into(), size: size.into(), bought: bought.into() }
}

fn row(item: &str, qty: &str, store: &str, bought: &str) -> Row {
    Row { item: item.into(), qty: qty.into(), store: store.into(), bought: bought.into() }
}

fn restock(date: &str) -> Command {
    Command::Restock { date: Some(date.into()), all: false }
}

fn digit_run(s: &str) -> bool {
    s.as_bytes().windows(4).any(|w| w.iter().all(|b| b.is_ascii_digit()))
}

#[test]
fn render_without_products_is_unchanged() {
    let rows = vec![row("Eggs", "12 ct", "Costco", "2026-11-01")];
    let text = pantry::render_with(&rows, &[], TODAY);
    assert!(text.ends_with("
**Hub:** [[Pantry]]
"), "{text}");
    assert!(!text.contains(pantry::PRODUCTS_OPEN), "{text}");
    assert_eq!(text, pantry::render(&rows, TODAY));
}

#[test]
fn products_round_trip_and_rows_unaffected() {
    let rows = vec![row("Eggs", "12 ct", "Costco", "2026-11-01")];
    let prods = vec![prod("Farmhouse Sourdough Bread", "Pepperidge Farm", "24 oz", "2026-11-08")];
    let text = pantry::render_with(&rows, &prods, TODAY);
    assert!(text.contains(pantry::PRODUCTS_OPEN) && text.contains(pantry::PRODUCTS_CLOSE), "{text}");
    assert_eq!(pantry::parse_products(&text), prods, "{text}");
    assert_eq!(pantry::parse_note(&text), rows, "{text}");
}

#[test]
fn extract_drops_extra_cells_and_prices() {
    let reply = "date: 2026-11-08\n- Farmhouse Sourdough Bread | Pepperidge Farm | 24 oz | $3.49 | extra\n- Mild Cheddar $2.99 | Good & Gather | 8 oz\n";
    let got = pantry::parse_extract(reply, "2026-11-08");
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0], prod("Farmhouse Sourdough Bread", "Pepperidge Farm", "24 oz", "2026-11-08"));
    assert_eq!(got[1].name, "Mild Cheddar", "{got:?}");
    for p in &got {
        for f in [&p.name, &p.brand, &p.size] {
            assert!(!f.contains('$') && !f.contains("3.49") && !f.contains("extra"), "{f}");
        }
    }
}

#[test]
fn solo_receipt_restock_writes_products() {
    let r = root("solo");
    receipt(&r, "2026-11-08", "Target receipt", "TARGET\nBREAD 3.49\nHAM 4.99");
    let ex = |p: &str| -> Option<String> {
        assert!(p.contains("TARGET"), "{p}");
        Some("date: 2026-11-08\n- Farmhouse Sourdough Bread | Pepperidge Farm | 24 oz\n- Uncured Black Forest Ham Ultra-Thin | Good & Gather | 16 oz\n".into())
    };
    let extract: &dyn Fn(&str) -> Option<String> = &ex;
    let out = pantry::run_with(&r, restock("2026-11-08"), TODAY, Some(extract)).unwrap();
    assert!(out.contains("Products:"), "{out}");
    let text = read(&r);
    let got = pantry::parse_products(&text);
    assert_eq!(got.len(), 2, "{text}");
    assert!(got.iter().any(|p| p.name == "Farmhouse Sourdough Bread" && p.brand == "Pepperidge Farm" && p.size == "24 oz" && p.bought == "2026-11-08"), "{text}");
    assert_eq!(pantry::products_at(&pantry_file(&r), TODAY).len(), 2);
}

#[test]
fn purchase_history_text_restocks_with_its_own_date() {
    let r = root("history");
    receipt(&r, "2026-11-10", "Purchase history", include_str!("fixtures/purchase_history_ocr.txt"));
    let ex = |p: &str| -> Option<String> {
        assert!(p.contains("Farmhouse Sourdough"), "{p}");
        Some("date: 2026-11-08\n- Farmhouse Sourdough Bread | Pepperidge Farm | 24 oz | $3.49\n- Uncured Black Forest Ham Ultra-Thin | Good & Gather | 16 oz\n".into())
    };
    let extract: &dyn Fn(&str) -> Option<String> = &ex;
    pantry::run_with(&r, restock("2026-11-10"), TODAY, Some(extract)).unwrap();
    let text = read(&r);
    let got = pantry::parse_products(&text);
    assert_eq!(got.len(), 2, "{text}");
    assert!(got.iter().all(|p| p.bought == "2026-11-08"), "the reply's date wins over the file date: {text}");
    for bad in ["$", "Order total", "4242", "Pat Example"] {
        assert!(!text.contains(bad), "{bad} reached the pantry note: {text}");
    }
}

#[test]
fn split_restock_calls_no_extractor_and_lists_only_mine() {
    let r = root("split");
    put(&r, &format!("{}/Costco Split — 2026-10-02.md", vaults::SPLITS), &split_note(&[("Eggs", "Lion"), ("Milk", "Ana")]));
    let ex = |_p: &str| -> Option<String> { panic!("a split note must not call the extractor") };
    let extract: &dyn Fn(&str) -> Option<String> = &ex;
    pantry::run_with(&r, restock("2026-10-02"), TODAY, Some(extract)).unwrap();
    let got = pantry::parse_products(&read(&r));
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].brand, "Costco");
    assert_eq!(got[0].bought, "2026-10-02");
    assert!(!read(&r).contains("Milk"), "{}", read(&r));
}

#[test]
fn out_of_keeps_the_block() {
    let r = root("out");
    let rows = vec![row("Eggs", "12 ct", "Costco", "2026-11-01"), row("Milk", "1 gal", "Costco", "2026-11-01")];
    let prods = vec![prod("Farmhouse Sourdough Bread", "Pepperidge Farm", "24 oz", "2026-11-08")];
    put(&r, vaults::PANTRY, &pantry::render_with(&rows, &prods, TODAY));
    pantry::run_with(&r, Command::OutOf("eggs".into()), TODAY, None).unwrap();
    let text = read(&r);
    let left = pantry::parse_note(&text);
    assert!(!left.iter().any(|r| r.item == "Eggs"), "{text}");
    assert!(left.iter().any(|r| r.item == "Milk"), "{text}");
    assert_eq!(pantry::parse_products(&text), prods, "{text}");
}

#[test]
fn products_at_cleans_ages_and_refuses_links() {
    let r = root("products-at");
    let note = r.join("made-up-pantry.md");
    let text = format!(
        "## Products\n\n{}\n- Milk · Store · 1 L · 2026-11-09\n- Old Thing · - · - · 2026-01-01\n- Future Thing · - · - · 2026-12-01\n- Bread $3.49 · Brand · 24 oz · 2026-11-08\n{}\n",
        pantry::PRODUCTS_OPEN,
        pantry::PRODUCTS_CLOSE
    );
    fs::write(&note, text).unwrap();
    let names: Vec<String> = pantry::products_at(&note, TODAY).into_iter().map(|p| p.name).collect();
    assert_eq!(names, vec!["Milk".to_string(), "Bread".to_string()], "newest first; old and future dropped; price cleaned");
    let link = r.join("linked-pantry.md");
    std::os::unix::fs::symlink(&note, &link).unwrap();
    assert!(pantry::products_at(&link, TODAY).is_empty(), "a linked note is refused");
    assert!(pantry::products_at(&r.join("missing.md"), TODAY).is_empty());
}

#[test]
fn restock_without_helper_matches_main() {
    let a = root("main-a");
    let b = root("main-b");
    for x in [&a, &b] {
        receipt(x, "2026-11-08", "Solo receipt", "ORG EGGS\nMLK");
    }
    let ua = pantry::run_with(&a, restock("2026-11-08"), TODAY, None).unwrap();
    let ub = pantry::run(&b, restock("2026-11-08"), TODAY).unwrap();
    assert_eq!(ua, ub);
    assert_eq!(read(&a), read(&b));
}

#[test]
fn pantry_never_holds_money_or_digit_runs() {
    for i in 0..50 {
        let r = root(&format!("money-{i}"));
        receipt(&r, "2026-11-08", "Solo receipt", "ORG EGGS");
        let reply = format!(
            "date: 2026-11-08\n- Item {i} $12.99 | Brand 4111222233334444 | 2 * 3 | x\n- Eggs {i}€ | Store | 12 ct\n- Pack £4 | ¥5 | 8 oz\n"
        );
        let ex = |_p: &str| -> Option<String> { Some(reply.clone()) };
        let extract: &dyn Fn(&str) -> Option<String> = &ex;
        pantry::run_with(&r, restock("2026-11-08"), TODAY, Some(extract)).unwrap();
        let text = read(&r).replace("**Hub:**", "Hub:");
        for bad in ["$", "€", "£", "¥", "*", "12.99", "4111"] {
            assert!(!text.contains(bad), "{bad} in {text}");
        }
        for p in pantry::parse_products(&text) {
            for f in [&p.name, &p.brand, &p.size] {
                assert!(!digit_run(f), "digit run in {f:?}");
                assert!(!f.contains('|'), "{f:?}");
            }
        }
    }
}

// Review round 1 (#161): a sized number keeps its size; a card run and a bare card group still drop.
#[test]
fn sized_numbers_keep_their_size() {
    assert_eq!(pantry::clean_field("Swanson Chicken Broth 1000 mL"), "Swanson Chicken Broth 1000 mL");
    assert_eq!(pantry::clean_field("Hummus 1.25 lb"), "Hummus 1.25 lb");
    assert_eq!(pantry::clean_field("Card 4321"), "Card");
    assert_eq!(pantry::clean_field("Order 1234567890 Eggs"), "Order Eggs");
}
