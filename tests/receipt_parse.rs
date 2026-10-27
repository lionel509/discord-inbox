//! Receipt text from the local OCR: classify it, read the printed time, clean the item lines. Synthetic text only.
use discord_inbox::receipt::{is_receipt, pick_time, prev_day, read, Stamp, DRINK_HINT};

const SAMPLE: &str = "CAMPUS DINING - EAST\n\
Order #4821\n\
10/26/2026 22:53\n\
2PC Chicken Combo\n\
>2PC Chicken  $ 11.45\n\
Spicy\n\
>Mashed Potato W- C Ajun Gra\n\
>Soda\n\
Subtotal  $ 11.45\n\
Tax  $ 0.00\n\
Total  $ 11.45\n\
Payment Source  Meal Plan\n\
Card #  XXXXXXXXXXXX4417\n\
Remaining Balance  $ 212.30";

#[test]
fn sample_receipt_reads_time_items_and_drinks() {
    assert!(is_receipt(SAMPLE));
    let r = read(SAMPLE);
    assert_eq!(r.printed, Some(("2026-10-26".to_string(), "22:53".to_string())));
    assert_eq!(r.items, vec!["2 pc fried chicken", "mashed potato with cajun gravy"]);
    assert_eq!(r.drinks, vec!["soda"]);
}

#[test]
fn printed_time_variants() {
    let pm = "10/26/26 10:53 PM\n>Chicken  $ 5.00\nTotal  $ 5.00";
    assert_eq!(read(pm).printed, Some(("2026-10-26".to_string(), "22:53".to_string())));
    let midnight = "10/26/2026 12:05 AM\n>Chicken  $ 5.00\nTotal  $ 5.00";
    assert_eq!(read(midnight).printed, Some(("2026-10-26".to_string(), "00:05".to_string())));
    let next_line = "10/26/2026\n22:53:10\n>Chicken  $ 5.00\nTotal  $ 5.00";
    assert_eq!(read(next_line).printed, Some(("2026-10-26".to_string(), "22:53".to_string())));
    let none = ">Chicken  $ 5.00\nTotal  $ 5.00";
    assert_eq!(read(none).printed, None);
}

#[test]
fn priced_mode_takes_only_priced_lines() {
    let t = "CAMPUS DINING\n10/26/2026 12:10\n2PC Chicken Combo\nSpicy\nChicken Tenders  $ 6.25\nTotal  $ 6.25";
    let r = read(t);
    assert_eq!(r.items, vec!["chicken tenders"]);
    assert!(r.drinks.is_empty());
}

#[test]
fn non_receipts_are_not_receipts() {
    assert!(!is_receipt(""));
    assert!(!is_receipt("GRILLED SALMON BOWL"));
    assert!(!is_receipt("Nutrition Facts\nCalories 250\nTotal Fat 8g 10%\nTotal Carbohydrate 30g"));
    assert!(!is_receipt("MENU\nChicken  $ 9.00\nSalmon  $ 12.50"));
}

#[test]
fn pick_time_covers_every_stamp() {
    let p = |d: &str, t: &str| Some((d.to_string(), t.to_string()));
    assert_eq!(
        pick_time(p("2026-10-27", "09:00"), "2026-10-27", "09:05"),
        ("2026-10-27".to_string(), "09:00".to_string(), Stamp::Printed)
    );
    // Ten minutes after the send time still counts; eleven does not.
    assert_eq!(pick_time(p("2026-10-27", "09:15"), "2026-10-27", "09:05").2, Stamp::Printed);
    assert_eq!(
        pick_time(p("2026-10-27", "09:16"), "2026-10-27", "09:05").2,
        Stamp::OutOfRange("2026-10-27 09:16".to_string())
    );
    assert_eq!(
        pick_time(p("2026-10-26", "22:53"), "2026-10-27", "09:12"),
        ("2026-10-26".to_string(), "22:53".to_string(), Stamp::PrintedYesterday)
    );
    assert_eq!(
        pick_time(p("2026-10-20", "12:00"), "2026-10-27", "09:12"),
        ("2026-10-27".to_string(), "09:12".to_string(), Stamp::OutOfRange("2026-10-20 12:00".to_string()))
    );
    assert_eq!(
        pick_time(None, "2026-10-27", "09:12"),
        ("2026-10-27".to_string(), "09:12".to_string(), Stamp::NotFound)
    );
}

#[test]
fn prev_day_crosses_months_and_years() {
    assert_eq!(prev_day("2026-03-01").as_deref(), Some("2026-02-28"));
    assert_eq!(prev_day("2026-01-01").as_deref(), Some("2025-12-31"));
    assert_eq!(prev_day("not a day"), None);
}

#[test]
fn drink_hint_names_the_missing_size() {
    assert!(DRINK_HINT.contains("regular soda 20 fl oz"));
}

/// std-only LCG, so the generated cases are the same on every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

#[test]
fn privacy_no_card_balance_order_or_totals_in_items_or_drinks() {
    let headers = ["CAMPUS DINING - EAST", "Order #NNNN", "Cashier: 12", "Thank you"];
    let items = ["2PC Chicken", "Mashed Potato W- C Ajun Gra", "Soda", "Fries", "Mac N Cheese", "Chicken Tenders"];
    let mods = ["Spicy", "Extra gravy", "Mild"];
    for seed in 0..1000u64 {
        let mut g = Lcg(seed);
        let card = format!("{:04}", 1000 + g.next(9000));
        let order = format!("{:04}", 1000 + g.next(9000));
        let bal = format!("{}.{:02}", 100 + g.next(900), g.next(100));
        let marked = g.next(2) == 0;
        let mut lines: Vec<String> = Vec::new();
        lines.push(headers[g.next(headers.len())].replace("NNNN", &order));
        lines.push(format!("10/26/2026 {:02}:{:02}", 10 + g.next(12), g.next(60)));
        for _ in 0..(1 + g.next(4)) {
            let it = items[g.next(items.len())];
            let price = format!("{}.{:02}", 1 + g.next(12), g.next(100));
            if marked {
                lines.push(format!(">{it}  $ {price}"));
            } else {
                lines.push(format!("{it}  $ {price}"));
            }
            if g.next(2) == 0 {
                lines.push(mods[g.next(mods.len())].to_string());
            }
        }
        lines.push(format!("Subtotal  $ {bal}"));
        lines.push(format!("Total  $ {bal}"));
        lines.push(format!("Card #  XXXXXXXXXXXX{card}"));
        lines.push(format!("Remaining Balance  $ {bal}"));
        let text = lines.join("\n");
        let r = read(&text);
        for s in r.items.iter().chain(r.drinks.iter()) {
            assert!(!s.contains(&card), "seed {seed}: card tail in {s:?}");
            assert!(!s.contains(&order), "seed {seed}: order number in {s:?}");
            assert!(!s.contains(&bal), "seed {seed}: balance in {s:?}");
            for w in ["card", "balance", "payment", "order", "total", "tax"] {
                assert!(!s.split_whitespace().any(|t| t == w), "seed {seed}: {w:?} in {s:?}");
            }
        }
    }
}
