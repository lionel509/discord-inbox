//! Pantry restock demo (#50): five steps on a temp vault, printing `On Hand.md` after each step.
//! Offline: no Discord, no model. Every folder lives under a temp dir that is removed at the end.
//!
//! Run: `cargo run --example pantry_demo`
use discord_inbox::pantry;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The sender's local date, fixed so the Days column is stable.
const TODAY: &str = "2026-10-15";
const PANTRY: &str = "Fidelity/Pantry/On Hand.md";
const SPLITS: &str = "State Street/Cash Flow/Spending/Splits";

fn put(root: &Path, rel: &str, text: &str) {
    let p: PathBuf = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// A receipt note as the bot files it: one `## OCR` block with the lines the OCR read.
fn receipt(lines: &[&str]) -> String {
    let mut s = String::from("## OCR 1.jpg\n\n~~~text\n");
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s.push_str("~~~\n");
    s
}

/// A finished Costco split note: the Item / Who table the bot reads.
fn split(rows: &[(&str, &str)]) -> String {
    let mut s = String::from("# Costco Split — 2026-10-13\n\n| Item | Qty | Who | Price |\n|---|---|---|---|\n");
    for (item, who) in rows {
        s.push_str(&format!("| {item} | 1 | {who} | 1.00 |\n"));
    }
    s.push_str("\nProse after the table.\n");
    s
}

fn on_hand(root: &Path) -> String {
    fs::read_to_string(root.join(PANTRY)).unwrap_or_else(|_| "(no On Hand.md yet)\n".into())
}

fn header(n: u32, title: &str) {
    println!("\n=== {n}. {title} ===");
}

/// Parses a DM line the way the bot does, runs it, and prints the reply (or the refusal).
fn say(root: &Path, line: &str) -> Result<String, String> {
    let cmd = pantry::command(line).unwrap_or_else(|| panic!("not a pantry command: {line}"));
    println!("> {line}");
    let out = pantry::run(root, cmd, TODAY);
    match &out {
        Ok(reply) => println!("-- reply --\n{reply}"),
        Err(e) => println!("-- refused --\n{e}"),
    }
    out
}

fn print_note(root: &Path) {
    println!("-- {PANTRY} --\n{}", on_hand(root));
}

fn main() -> ExitCode {
    let root = std::env::temp_dir().join(format!("discord-inbox-pantry-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("Fidelity/Pantry")).unwrap();
    fs::create_dir_all(root.join(SPLITS)).unwrap();
    fs::create_dir_all(root.join("State Street")).unwrap();

    // 1. Empty pantry, solo grocery receipt. The charger is on the receipt and must not reach the pantry.
    header(1, "Empty pantry, solo grocery receipt (USB charger on the same receipt)");
    put(
        &root,
        "State Street/2026-10-05 Grocer receipt.md",
        &receipt(&["GRN COD FRESH 14.98", "ATL SALMON 12.49", "ORG EGGS 5.99", "GRN SCALLIONS 1.29", "USB CHARGER 19.99", "TOTAL 54.74"]),
    );
    if say(&root, "restock 2026-10-05").is_err() {
        eprintln!("step 1 unexpectedly refused");
        return ExitCode::FAILURE;
    }
    print_note(&root);
    let s1 = on_hand(&root);
    if s1.contains("charger") || !s1.contains("| cod |") || !s1.contains("| scallion |") {
        eprintln!("step 1 did not behave as described");
        return ExitCode::FAILURE;
    }

    // 2. A second receipt rebuying salmon a few days later: the salmon row is replaced, not duplicated.
    header(2, "Rebuy salmon at another store a week later (2026-10-12)");
    put(&root, "State Street/2026-10-12 Corner Market receipt.md", &receipt(&["ATL SALMON 13.49", "TOTAL 13.49"]));
    if say(&root, "restock 2026-10-12").is_err() {
        eprintln!("step 2 unexpectedly refused");
        return ExitCode::FAILURE;
    }
    print_note(&root);
    if on_hand(&root).matches("| salmon |").count() != 1 {
        eprintln!("step 2 did not behave as described");
        return ExitCode::FAILURE;
    }

    // 3. Costco receipt with a split note. Lionel's rows and shares only: Danny's beef is skipped.
    header(3, "Costco receipt with split note (Who: Lionel / Danny ½ · Lionel ½ / Danny)");
    put(&root, "State Street/2026-10-13 Costco receipt.md", &receipt(&["COSTCO WHOLESALE", "KS COD 22.00", "KS EGGS 11.99", "KS BEEF 31.00", "TOTAL 65.00"]));
    put(
        &root,
        &format!("{SPLITS}/Costco Split — 2026-10-13.md"),
        &split(&[("COD — cod", "Lionel"), ("ORG EGGS — eggs", "Danny ½ · Lionel ½"), ("KS BEEF — beef", "Danny")]),
    );
    if say(&root, "restock 2026-10-13").is_err() {
        eprintln!("step 3 unexpectedly refused");
        return ExitCode::FAILURE;
    }
    print_note(&root);
    let s3 = on_hand(&root);
    if s3.contains("beef") || !s3.contains("| eggs | ½ |") || !s3.contains("| cod |  | Costco | 2026-10-13 |") {
        eprintln!("step 3 did not behave as described");
        return ExitCode::FAILURE;
    }

    // 4. Costco receipt with NO split note: refused, and nothing is written.
    header(4, "Costco receipt with NO split note (2026-10-14)");
    put(&root, "State Street/2026-10-14 Costco receipt.md", &receipt(&["COSTCO WHOLESALE", "KS SALMON 24.99", "TOTAL 24.99"]));
    let before = on_hand(&root);
    let refused = say(&root, "restock 2026-10-14").is_err();
    let unchanged = on_hand(&root) == before;
    println!("-- {PANTRY} unchanged: {unchanged} --");
    if !refused || !unchanged {
        eprintln!("step 4 did not behave as described");
        return ExitCode::FAILURE;
    }

    // 5. Drop a row by command.
    header(5, "\"out of cod\"");
    if say(&root, "out of cod").is_err() {
        eprintln!("step 5 unexpectedly refused");
        return ExitCode::FAILURE;
    }
    print_note(&root);
    if on_hand(&root).contains("| cod |") {
        eprintln!("step 5 did not behave as described");
        return ExitCode::FAILURE;
    }

    let _ = fs::remove_dir_all(&root);
    println!("\nAll five steps behaved as described.");
    ExitCode::SUCCESS
}
