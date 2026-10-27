//! Pip meal-line demo: the same DM lines through `nutrition::parse_meal`, printing what each piece
//! would log (food, grams, unit, assumed or typed) or the refusal hint. Pure parsing: no folders, no
//! Discord. Uses only APIs that exist on both the PR head and origin/main, so scripts/demo-pip-v2.sh
//! can compile this same file against main for the before/after view.
use discord_inbox::nutrition;

/// The DM lines, as he would type them.
const LINES: [&str; 13] = [
    "1 1/2 lb salmon",
    "2 lb 4 oz salmon",
    "1 banana (200 g)",
    "two eggs",
    "half an avocado",
    "salmon x2",
    "2 x 150 g salmon",
    "salmon 2 servings",
    "oats ~150 g",
    "salmon 5~6 oz",
    "salmon at 7pm 150 g",
    "salmon 1,000 g",
    "salmon 1,5 kg",
];

fn main() {
    for line in LINES {
        println!("> {line}");
        let parsed = nutrition::parse_meal(line);
        if parsed.items.is_empty() && parsed.unknown.is_empty() {
            println!("  (nothing logged)");
        }
        for item in &parsed.items {
            let how = if item.assumed { "assumed" } else { "as typed" };
            println!(
                "  logged   {}  {} g  (qty {:.2}, unit {:?}, {how})",
                item.food.name,
                item.grams.round(),
                item.qty,
                item.unit
            );
        }
        for u in &parsed.unknown {
            println!("  refused  {}", u.hint);
            println!("           piece: \"{}\"", u.text);
        }
        println!();
    }
}
