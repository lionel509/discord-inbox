// Pantry restock (#50): unit tests on made-up folders and fixtures. Never real vault data.
use discord_inbox::pantry::{self, Command, Row};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

const PANTRY: &str = "Fidelity/Pantry/On Hand.md";
const SPLIT: &str = "State Street/Cash Flow/Spending/Splits";

fn root(name: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("di-pantry-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("Fidelity/Pantry")).unwrap();
    fs::create_dir_all(dir.join(SPLIT)).unwrap();
    fs::create_dir_all(dir.join("State Street")).unwrap();
    dir
}

fn put(root: &Path, rel: &str, text: &str) {
    fs::write(root.join(rel), text).unwrap();
}

fn read(root: &Path) -> String {
    fs::read_to_string(root.join(PANTRY)).unwrap_or_default()
}

fn split_note(who_rows: &[(&str, &str)]) -> String {
    let mut s = String::from("# Costco Split — 2026-10-02\n\n| Item | Qty | Who | Price |\n|---|---|---|---|\n");
    for (item, who) in who_rows {
        s.push_str(&format!("| {item} | 1 | {who} | 1.00 |\n"));
    }
    s.push_str("\nProse after the table.\n");
    s
}

fn items_of(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| r.item.clone()).collect()
}

#[test]
fn command_parses_restock_variants_and_out_of() {
    assert_eq!(pantry::command("restock"), Some(Command::Restock { date: None, all: false }));
    assert_eq!(pantry::command("Restock all"), Some(Command::Restock { date: None, all: true }));
    assert_eq!(
        pantry::command("restock 2026-10-02 all"),
        Some(Command::Restock { date: Some("2026-10-02".into()), all: true })
    );
    assert_eq!(pantry::command("out of cod"), Some(Command::OutOf("cod".into())));
    assert_eq!(pantry::command("out of the salmon."), Some(Command::OutOf("salmon".into())));
    for chat in ["out of milk?", "restock the shelves tmr", "restocking", "restock 2026-13"] {
        assert_eq!(pantry::command(chat), None, "{chat} must reach the model");
    }
}

#[test]
fn food_matches_whole_words_only() {
    assert_eq!(pantry::food("KS ATL SALMON"), Some("salmon"));
    assert_eq!(pantry::food("ORG EGGS"), Some("eggs"));
    assert_eq!(pantry::food("`KSULTFILTMLK` — ultra-filtered milk"), Some("milk"));
    assert_eq!(pantry::food("mlk"), Some("milk"));
    for junk in ["PRICE", "VEGGIE TRAY", "MRS MEYERS", "CODE 1234", "TOTAL"] {
        assert_eq!(pantry::food(junk), None, "{junk}");
    }
}

#[test]
fn split_keeps_only_rows_whose_who_names_him() {
    let text = split_note(&[
        ("KS ATL SALMON — salmon", "Lionel ½ · Ana ½"),
        ("ORG EGGS — eggs", "lionle 1/3 Ben 2/3"),
        ("KSULTFILTMLK — milk", "Lionel"),
        ("COD — cod", "Lionel"),
        ("NAUTICAPJ2PK — pyjamas", "Lionel"),
        ("BEEF — beef", "Ben"),
        ("BUTTER — butter", "Ana ½ · Ben ½"),
        ("RICE — rice", ""),
        ("TOTAL — total", "Ana"),
    ]);
    let s = pantry::parse_split(&text);
    let mut mine = s.mine.clone();
    mine.sort();
    assert_eq!(
        mine,
        vec![
            ("cod".to_string(), String::new()),
            ("eggs".to_string(), "1/3".to_string()),
            ("milk".to_string(), String::new()),
            ("salmon".to_string(), "½".to_string()),
        ]
    );
    assert_eq!(s.others, 2, "beef and butter belong to other people");
    assert_eq!(s.unassigned, vec!["rice".to_string()]);
}

#[test]
fn receipt_items_reads_only_ocr_blocks() {
    let note = "---\nreceived: 2026-10-02T15:00:00Z\n---\ngot rice\n\n## OCR 1.jpg\n\n~~~text\nKS ATL SALMON\nPRICE 12.00\nMRS MEYERS\nORG EGGS\n~~~\n";
    assert_eq!(pantry::receipt_items(note), vec!["salmon".to_string(), "eggs".to_string()]);
}

#[test]
fn upsert_replaces_same_item_and_sorts_oldest_first() {
    let row = |item: &str, bought: &str, store: &str| Row {
        item: item.into(),
        qty: String::new(),
        store: store.into(),
        bought: bought.into(),
    };
    let mut rows = vec![row("milk", "2026-09-01", "Old"), row("cod", "2026-09-20", "Costco")];
    pantry::upsert(&mut rows, vec![row("milk", "2026-10-02", "Costco"), row("salmon", "2026-10-02", "Costco")]);
    assert_eq!(rows.len(), 3);
    let milk = rows.iter().find(|r| r.item == "milk").unwrap();
    assert_eq!((milk.bought.as_str(), milk.store.as_str()), ("2026-10-02", "Costco"));
    let dates: Vec<&str> = rows.iter().map(|r| r.bought.as_str()).collect();
    let mut sorted = dates.clone();
    sorted.sort();
    assert_eq!(dates, sorted);
}

#[test]
fn render_parse_round_trip_recomputes_days() {
    assert_eq!(pantry::days("2026-10-02", "2026-10-15"), Some(13));
    assert_eq!(pantry::days("garbage", "2026-10-15"), None);
    let rows = vec![
        Row { item: "cod".into(), qty: String::new(), store: "Costco".into(), bought: "2026-10-02".into() },
        Row { item: "tofu".into(), qty: String::new(), store: String::new(), bought: "garbage".into() },
    ];
    let text = pantry::render(&rows, "2026-10-15");
    assert!(text.contains("| cod |"), "{text}");
    assert!(text.contains("| 13 |"), "{text}");
    assert!(text.contains("| ? |"), "{text}");
    assert!(text.contains("[[Pantry]]"), "{text}");
    let back = pantry::parse_note(&text);
    assert_eq!(items_of(&back), vec!["cod".to_string(), "tofu".to_string()]);
    assert_eq!(back[0].bought, "2026-10-02");
}

#[test]
fn restock_prefers_split_over_receipt_and_creates_note() {
    let r = root("split");
    put(&r, &format!("{SPLIT}/Costco Split — 2026-10-02.md"), &split_note(&[
        ("KS ATL SALMON — salmon", "Lionel ½ · Ana ½"),
        ("COD — cod", "Lionel"),
        ("BEEF — beef", "Ben"),
    ]));
    put(&r, "State Street/2026-10-02 Costco receipt.md", "## OCR 1.jpg\n\n~~~text\nCOSTCO WHOLESALE\nBEEF 12.00\n~~~\n");
    let rep = pantry::restock(&r, "2026-10-02", false).expect("split restock");
    assert!(rep.added.contains(&"salmon".to_string()));
    assert!(rep.added.contains(&"cod".to_string()));
    assert!(!rep.added.contains(&"beef".to_string()));
    let note = read(&r);
    assert!(note.contains("| salmon |"), "{note}");
    assert!(note.contains("[[Pantry]]"), "{note}");
    assert!(!note.contains("beef"), "{note}");
}

#[test]
fn restock_receipt_mode_when_no_split() {
    let r = root("receipt");
    put(&r, "State Street/2026-10-03 Solo receipt.md", "## OCR 1.jpg\n\n~~~text\nORG EGGS\nMLK\n~~~\n");
    let rep = pantry::restock(&r, "2026-10-03", false).expect("receipt restock");
    assert!(rep.added.contains(&"eggs".to_string()));
    assert!(rep.added.contains(&"milk".to_string()));
    assert!(read(&r).contains("| eggs |"));
}

#[test]
fn restock_refuses_costco_ocr_without_split_unless_all() {
    let r = root("costco");
    put(&r, "State Street/2026-10-04 Costco receipt.md", "## OCR 1.jpg\n\n~~~text\nCOSTCO WHOLESALE\nORG EGGS\n~~~\n");
    let err = pantry::restock(&r, "2026-10-04", false).expect_err("costco without split must refuse");
    assert!(err.contains("split"), "{err}");
    assert!(!r.join(PANTRY).exists());
    let rep = pantry::restock(&r, "2026-10-04", true).expect("restock all");
    assert!(rep.added.contains(&"eggs".to_string()));
}

#[test]
fn restock_twice_is_idempotent() {
    let r = root("twice");
    put(&r, "State Street/2026-10-05 Solo receipt.md", "## OCR 1.jpg\n\n~~~text\nORG EGGS\n~~~\n");
    pantry::restock(&r, "2026-10-05", false).unwrap();
    let first = read(&r);
    pantry::restock(&r, "2026-10-05", false).unwrap();
    assert_eq!(read(&r), first);
    assert_eq!(first.matches("| eggs |").count(), 1);
}

#[test]
fn out_of_drops_row_and_reports_missing() {
    let r = root("outof");
    put(&r, &format!("{SPLIT}/Costco Split — 2026-10-02.md"), &split_note(&[("COD — cod", "Lionel"), ("KS ATL SALMON — salmon", "Lionel")]));
    pantry::restock(&r, "2026-10-02", false).unwrap();
    let out = pantry::out_of(&r, "the cod.", "2026-10-15").unwrap();
    assert!(out.starts_with("Dropped cod"), "{out}");
    assert!(!read(&r).contains("| cod |"));
    assert!(read(&r).contains("| salmon |"));
    let missing = pantry::out_of(&r, "milk", "2026-10-15").unwrap();
    assert!(missing.starts_with("No milk"), "{missing}");
}

#[cfg(unix)]
#[test]
fn restock_refuses_symlinked_note() {
    let r = root("symlink");
    put(&r, "State Street/2026-10-06 Solo receipt.md", "## OCR 1.jpg\n\n~~~text\nORG EGGS\n~~~\n");
    put(&r, "elsewhere.md", "keep me\n");
    std::os::unix::fs::symlink(r.join("elsewhere.md"), r.join(PANTRY)).unwrap();
    assert!(pantry::restock(&r, "2026-10-06", false).is_err());
    assert_eq!(fs::read_to_string(r.join("elsewhere.md")).unwrap(), "keep me\n");
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

fn has_lion(who: &str) -> bool {
    who.split(|c: char| c.is_whitespace() || "·,+&".contains(c))
        .any(|t| t.to_ascii_lowercase().starts_with("lion"))
}

#[test]
fn prop_only_his_food_rows_enter() {
    let items = [
        "COD — cod", "SALMON — salmon", "MLK — milk", "EGG — eggs", "TOFU — tofu", "RICE — rice", "BEEF — beef",
        "NAUTICAPJ2PK — pyjamas", "TOTAL — total", "CASH — cash", "MRS MEYERS — soap", "PRICE 9.99",
    ];
    let whos = ["Lionel", "lionle", "Lionel ½ · Ana ½", "lionel 1/3 ben 2/3", "Ana ½ · Ben ½", "ben", "Ana", ""];
    let mut rng = Lcg(50);
    for _ in 0..200 {
        let mut src: Vec<(&str, &str)> = Vec::new();
        for _ in 0..(1 + rng.next(8)) {
            src.push((items[rng.next(items.len())], whos[rng.next(whos.len())]));
        }
        let s = pantry::parse_split(&split_note(&src));
        for (canon, _share) in &s.mine {
            assert!(pantry::FOODS.iter().any(|(_, c)| c == canon), "{canon} is not canonical");
            assert!(src.iter().any(|(i, w)| pantry::food(i) == Some(canon.as_str()) && has_lion(w)), "{canon} came from a row not naming him");
        }
        for canon in &s.unassigned {
            assert!(src.iter().any(|(i, w)| pantry::food(i) == Some(canon.as_str()) && w.trim().is_empty()));
        }
    }
}

#[test]
fn prop_note_never_holds_raw_text() {
    let lines = ["KS ATL SALMON", "ORG EGGS", "MRS MEYERS", "PRICE 12.00", "NAUTICAPJ2PK", "KSULTFILTMLK", "TOTAL 33.10", "CASH 40.00"];
    let mut rng = Lcg(54);
    let mut with_rows = 0;
    for i in 0..100 {
        let r = root(&format!("prop{i}"));
        let mut ocr = String::from("## OCR 1.jpg\n\n~~~text\n");
        let mut used: Vec<&str> = Vec::new();
        for _ in 0..(1 + rng.next(6)) {
            let l = lines[rng.next(lines.len())];
            used.push(l);
            ocr.push_str(l);
            ocr.push('\n');
        }
        ocr.push_str("~~~\n");
        put(&r, "State Street/2026-10-07 Solo receipt.md", &ocr);
        pantry::restock(&r, "2026-10-07", false).unwrap();
        let note = read(&r);
        let rows = pantry::parse_note(&note);
        if !rows.is_empty() {
            with_rows += 1;
        }
        for row in &rows {
            assert!(pantry::FOODS.iter().any(|(_, c)| *c == row.item), "{} is not canonical", row.item);
        }
        for l in &used {
            assert!(!note.contains(l), "raw line {l} leaked into the note");
        }
    }
    assert!(with_rows > 0);
}

#[test]
fn demo_restock_prints_before_and_after() {
    let r = root("demo");
    let fixture = split_note(&[
        ("KS ATL SALMON — salmon", "Lionel ½ · Ana ½"),
        ("COD — cod", "Lionel"),
        ("BEEF — beef", "Ben"),
    ]);
    put(&r, &format!("{SPLIT}/Costco Split — 2026-10-02.md"), &fixture);
    println!("--- split note (fixture) ---\n{fixture}");
    let rep = pantry::restock(&r, "2026-10-02", false).unwrap();
    println!("--- reply ---\n{}", rep.text());
    println!("--- {PANTRY} ---\n{}", read(&r));
}

// Review of #58 (made-up fixtures only).

#[test]
fn out_of_takes_only_whole_food_items() {
    assert_eq!(pantry::command("out of eggs and milk"), Some(Command::OutOf("eggs and milk".into())));
    assert_eq!(pantry::command("out of the salmon."), Some(Command::OutOf("salmon".into())));
    for chat in ["out of orange juice", "out of paper towels", "out of town till sunday", "out of peanut butter"] {
        assert_eq!(pantry::command(chat), None, "{chat} must reach the model");
    }
}

#[test]
fn out_of_two_items_drops_both_and_keeps_the_rest() {
    let r = root("outtwo");
    put(&r, &format!("{SPLIT}/Costco Split — 2026-10-02.md"), &split_note(&[("EGGS", "Lionel"), ("MILK", "Lionel"), ("ORANGE — orange", "Lionel")]));
    pantry::restock(&r, "2026-10-02", false).unwrap();
    let out = pantry::run(&r, pantry::command("out of eggs and milk").unwrap(), "2026-10-15").unwrap();
    assert!(out.starts_with("Dropped"), "{out}");
    let note = read(&r);
    assert!(!note.contains("| eggs |") && !note.contains("| milk |"), "{note}");
    assert!(note.contains("| orange |"), "{note}");
}

#[test]
fn out_of_refuses_a_multiword_item_instead_of_dropping_its_first_word() {
    let r = root("outjuice");
    put(&r, &format!("{SPLIT}/Costco Split — 2026-10-02.md"), &split_note(&[("ORANGE", "Lionel")]));
    pantry::restock(&r, "2026-10-02", false).unwrap();
    assert!(pantry::out_of(&r, "orange juice", "2026-10-15").is_err());
    assert!(read(&r).contains("| orange |"));
}

#[test]
fn food_matches_es_plurals() {
    assert_eq!(pantry::food("TOMATOES ROMA 3LB"), Some("tomato"));
    assert_eq!(pantry::food("Potatoes 5lb"), Some("potato"));
    assert_eq!(pantry::food("CABBAGES"), Some("cabbage"));
}

#[test]
fn receipt_tender_lines_are_not_food() {
    let note = "## OCR 1.jpg\n\n~~~text\nAPPLE PAY 12.00\nVISA ENDING 1234\nORG EGGS\n~~~\n";
    assert_eq!(pantry::receipt_items(note), vec!["eggs".to_string()]);
}

#[test]
fn receipt_store_is_the_note_each_row_came_from() {
    let r = root("perrow");
    put(&r, "State Street/2026-10-06 Grocer receipt.md", "## OCR 1.jpg\n\n~~~text\nKS ATL SALMON\n~~~\n");
    put(&r, "State Street/2026-10-06 Card statement.md", "Statement for Visa ending 1234, merchant ACME.\n");
    let rep = pantry::restock(&r, "2026-10-06", false).expect("receipt restock");
    let note = read(&r);
    assert!(note.contains("| salmon |  | Grocer receipt |"), "{note}");
    assert!(!note.contains("Card statement"), "{note}");
    assert!(!rep.text().contains("Card statement"), "{}", rep.text());
}

#[test]
fn receipt_day_with_no_food_says_so() {
    let r = root("nofood");
    put(&r, "State Street/2026-10-07 Card statement.md", "Statement for Visa ending 1234.\n");
    let rep = pantry::restock(&r, "2026-10-07", false).expect("restock");
    assert!(rep.text().starts_with("Nothing food-shaped"), "{}", rep.text());
    assert!(!r.join(PANTRY).exists());
}

#[test]
fn upsert_keeps_the_newer_purchase_date() {
    let row = |bought: &str| Row { item: "milk".into(), qty: String::new(), store: "Costco".into(), bought: bought.into() };
    let mut rows = vec![row("2026-10-08")];
    pantry::upsert(&mut rows, vec![row("2026-10-01")]);
    assert_eq!(rows[0].bought, "2026-10-08");
}

#[test]
fn out_of_accepts_oxford_comma_and_es_plurals_and_refuses_question_bang() {
    assert!(matches!(pantry::command("out of eggs, milk, and bread"), Some(Command::OutOf(_))));
    assert!(matches!(pantry::command("out of tomatoes"), Some(Command::OutOf(_))));
    assert_eq!(pantry::command("out of milk?!"), None);
    let r = root("outoxford");
    put(&r, &format!("{SPLIT}/Costco Split — 2026-10-02.md"), &split_note(&[("EGGS", "Lionel"), ("MILK", "Lionel"), ("BREAD", "Lionel"), ("TOMATO", "Lionel")]));
    pantry::restock(&r, "2026-10-02", false).unwrap();
    pantry::run(&r, pantry::command("out of eggs, milk, and bread").unwrap(), "2026-10-15").unwrap();
    pantry::run(&r, pantry::command("out of tomatoes").unwrap(), "2026-10-15").unwrap();
    let note = read(&r);
    for f in ["eggs", "milk", "bread", "tomato"] {
        assert!(!note.contains(&format!("| {f} |")), "{note}");
    }
}

#[test]
fn out_of_milk_keeps_a_row_sharing_its_initial() {
    let r = root("outinitial");
    put(&r, &format!("{SPLIT}/Costco Split — 2026-10-02.md"), &split_note(&[("MILK", "Lionel"), ("MUSHROOM", "Lionel")]));
    pantry::restock(&r, "2026-10-02", false).unwrap();
    pantry::run(&r, pantry::command("out of milk").unwrap(), "2026-10-15").unwrap();
    let note = read(&r);
    assert!(!note.contains("| milk |") && note.contains("| mushroom |"), "{note}");
}

#[test]
fn upsert_refreshes_a_row_whose_bought_cell_is_not_iso() {
    let mut rows = vec![Row { item: "cod".into(), qty: "".into(), store: "A".into(), bought: "Oct 2".into() }];
    pantry::upsert(&mut rows, vec![Row { item: "cod".into(), qty: "".into(), store: "B".into(), bought: "2026-10-05".into() }]);
    assert_eq!(rows[0].store, "B");
}

#[test]
fn b3_ocr_misread_costco_header_still_refused_without_split() {
    let r = root("b3");
    put(&r, "State Street/2026-10-04 Receipt.md", "## OCR 1.jpg\n\n~~~text\nC0STC0 WHOLESALE\nORG EGGS\n~~~\n");
    let err = pantry::restock(&r, "2026-10-04", false).expect_err("must refuse");
    assert!(err.contains("split"), "{err}");
}

#[test]
fn b4_restock_with_trailing_period_is_a_command() {
    assert_eq!(pantry::command("restock."), Some(Command::Restock { date: None, all: false }));
    assert_eq!(
        pantry::command("restock 2026-10-02."),
        Some(Command::Restock { date: Some("2026-10-02".into()), all: false })
    );
}

#[test]
fn b5_hand_added_three_cell_row_survives_restock() {
    let r = root("b5");
    put(&r, PANTRY, "| Item | Qty | Store | Bought |\n|---|---|---|---|\n| rice | 1 | Home |\n");
    put(&r, "State Street/2026-10-05 Solo receipt.md", "## OCR 1.jpg\n\n~~~text\nORG EGGS\n~~~\n");
    pantry::restock(&r, "2026-10-05", false).unwrap();
    assert!(read(&r).contains("| rice |"), "{}", read(&r));
}

#[test]
fn r6_b4_kirkland_address_on_plain_receipt_is_not_costco() {
    let r = root("r6b4");
    put(&r, "State Street/2026-10-06 Receipt.md", "## OCR 1.jpg\n\n~~~text\nFRESH MART\n12 MAIN ST KIRKLAND WA\nORG EGGS\n~~~\n");
    assert!(pantry::restock(&r, "2026-10-06", false).is_ok());
}

// Round 8 (decision: Costco is detected by "costco" alone; no Kirkland heuristic).
#[test]
fn r8_spaced_costco_header_without_split_is_refused() {
    let r = root("r8spaced");
    put(&r, "State Street/2026-10-04 Receipt.md", "## OCR 1.jpg\n\n~~~text\nC O S T C O  W H O L E S A L E\nORG EGGS\n~~~\n");
    let err = pantry::restock(&r, "2026-10-04", false).expect_err("spaced costco must refuse");
    assert!(err.contains("split"), "{err}");
}

#[test]
fn r8_kirkland_street_address_on_plain_receipt_is_not_costco() {
    let r = root("r8addr");
    put(&r, "State Street/2026-10-04 Receipt.md", "## OCR 1.jpg\n\n~~~text\nFRESH MART\n12 MAIN ST KIRKLAND WA\nORG EGGS\n~~~\n");
    let rep = pantry::restock(&r, "2026-10-04", false).expect("plain receipt is stocked");
    assert!(rep.added.contains(&"eggs".to_string()));
}

#[test]
fn r8_kirkland_sign_store_names_are_not_costco() {
    for (name, store) in [("sign", "KIRKLAND SIGN CO"), ("signal", "KIRKLAND SIGNAL WAY")] {
        let r = root(&format!("r8{name}"));
        put(&r, "State Street/2026-10-04 Receipt.md", &format!("## OCR 1.jpg\n\n~~~text\n{store}\nORG EGGS\n~~~\n"));
        let rep = pantry::restock(&r, "2026-10-04", false).unwrap_or_else(|e| panic!("{store}: {e}"));
        assert!(rep.added.contains(&"eggs".to_string()), "{store}");
    }
}

#[test]
fn r8_mixed_case_costco_without_split_is_refused() {
    let r = root("r8mixed");
    put(&r, "State Street/2026-10-04 Receipt.md", "## OCR 1.jpg\n\n~~~text\nCostco Wholesale\nORG EGGS\n~~~\n");
    let err = pantry::restock(&r, "2026-10-04", false).expect_err("mixed-case costco must refuse");
    assert!(err.contains("split"), "{err}");
}
