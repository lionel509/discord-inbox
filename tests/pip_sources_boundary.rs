//! #159 U4, INV-1: the reader's prompt holds only the four product fields. Planted money, card digits and a made-up
//! name sit in the pantry note's table, prose and extra block parts, and in a sibling receipt note; none may reach the prompt.
use discord_inbox::pantry::{self, Product};
use discord_inbox::pip_model::{self, Sources};
use std::fs;
use std::path::PathBuf;

#[test]
fn prompt_holds_only_product_fields() {
    let root = std::env::temp_dir().join(format!("di-sources-boundary-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("Kitchen/Pantry")).unwrap();
    fs::create_dir_all(root.join("Money")).unwrap();
    for i in 0..200 {
        let price = format!("${}.{:02}", 40 + i, i % 100);
        let card = format!("4111 2222 3333 {i:04}");
        let person = format!("Pat Example{i}");
        let note = format!(
            "# On Hand\n\n| Item | Qty | Store | Bought |\n|---|---|---|---|\n| Eggs | 12 ct | Costco {price} {card} {person} | 2026-11-01 |\n\nPaid {price} with {card}, thanks {person}.\n\n## Products\n\n{open}\n- Organic Eggs · Costco · 12 ct · 2026-11-01\n- Bread · Brand · {price} · 2026-11-08\n{close}\n\n**Hub:** [[{person}]]\n",
            open = pantry::PRODUCTS_OPEN,
            close = pantry::PRODUCTS_CLOSE,
        );
        let path = root.join("Kitchen/Pantry/On Hand.md");
        fs::write(&path, &note).unwrap();
        fs::write(root.join("Money/2026-11-08 Receipt.md"), format!("## OCR 1.jpg\n\n~~~text\nBREAD {price}\nCARD {card}\n{person}\n~~~\n")).unwrap();
        let products: Vec<Product> = pantry::products_at(&path, "2026-11-10");
        assert!(!products.is_empty(), "the products block is read");
        let src = Sources { foods: Vec::new(), dishes: Vec::new(), products };
        let photos: Vec<PathBuf> = Vec::new();
        let prompt = pip_model::prompt_sources("2 eggs and a slice of bread", &photos, &src);
        assert!(prompt.contains("Organic Eggs"), "{prompt}");
        for planted in [price.as_str(), card.as_str(), person.as_str(), "4111", "Pat Example"] {
            assert!(!prompt.contains(planted), "{planted} reached the prompt at {i}");
        }
    }
    let _ = fs::remove_dir_all(&root);
}
