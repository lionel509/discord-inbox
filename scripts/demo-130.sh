#!/bin/sh
# Pip meal-reader demo (#130, PR #133): the real noodle DM, a typed-weight DM whose reply leaves out grams, and a
# dotted-price receipt with card numbers in four joined forms. Part 1 and the receipt run on origin/main too (table
# only; main's own card masking), so before and after print side by side. The after run is this head through
# pip::handle with a /bin/sh stub as the meal reader: it prints a fixed fixture reply, so the run is deterministic
# and free. No model or key is called. Everything runs in scratch folders; no Discord, no Keychain, no real vault.
# Run from anywhere: sh scripts/demo-130.sh   (CARGO_TARGET_DIR optional; default ~/.cache/cargo-target)
set -e
REPO=$(cd "$(dirname "$0")/.." && pwd)
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/cargo-target}"
ROOT=$(mktemp -d "${TMPDIR:-/tmp}/demo-130.XXXXXX")
trap 'rm -rf "$ROOT"' EXIT
case "$ROOT" in
  "$HOME"/Documents*|"$HOME"/documents*) echo "refusing: scratch folder under ~/Documents" >&2; exit 1 ;;
esac
CACHE="$HOME/.cache/demo-130"
mkdir -p "$ROOT/Private/Nutrition" "$ROOT/fixtures" "$CACHE"

# Reader replies (the stub's fixtures). The noodle reply is the reader's answer to the real DM; the typed reply
# leaves grams out, so the weight he typed is the one logged.
cat > "$ROOT/fixtures/noodle.json" <<'JSON'
{"items":[{"name":"cooked egg noodles","grams":100,"source":"text","kcal_100":138,"protein_100":4.5,"carbs_100":25,"fat_100":2.1,"veg":false},{"name":"cooked chicken breast","grams":100,"source":"text","kcal_100":165,"protein_100":31,"carbs_100":0,"fat_100":3.6,"veg":false}],"unreadable":[]}
JSON
cat > "$ROOT/fixtures/typed.json" <<'JSON'
{"items":[{"name":"cooked egg noodles","source":"text","kcal_100":138,"protein_100":4.5,"carbs_100":25,"fat_100":2.1,"veg":false}],"unreadable":[]}
JSON
# The stub: reads the prompt, prints whatever reply the driver put in model-out.
printf '#!/bin/sh\ncat >/dev/null\ncat "%s"\n' "$ROOT/model-out" > "$ROOT/stub.sh"
chmod 755 "$ROOT/stub.sh"

# Driver for origin/main: pure parsing and masking, the only APIs main has in this shape.
cat > "$ROOT/demo130_before.rs" <<'RS'
//! Before: origin/main, table-only parse (no reader), the same DM lines and receipt.
use discord_inbox::{insight, nutrition, redact};

const NOODLE: &str = "100 grnas of noodle 100 grams of chicken";
const TYPED: &str = "200g noodles";
const RECEIPT: [&str; 8] = [
    "CORNER MART  store 0042",
    "Oat milk 1 L      4.99",
    "Bananas x6       12.50",
    "Total EUR     1.234,50",
    "Card 4111 1111 1111 1111",
    "Card 4111-1111-1111-1111",
    "Card 4111/1111/1111/1111",
    "Card 4111\u{200B}1111\u{200B}1111\u{200B}1111",
];

fn show(text: &str) {
    println!("> {text}");
    let p = nutrition::parse_meal(text);
    for it in &p.items {
        println!("  logged   {}", nutrition::item_line(it));
    }
    for u in &p.unknown {
        println!("  refused  {} ({})", u.text, u.hint);
    }
    let m = nutrition::macros(&p.items);
    println!("  total    {:.0} kcal · {:.1} g protein · {:.1} g carbs · {:.1} g fat · fiber not tracked", m.kcal, m.protein, m.carbs, m.fat);
}

fn main() {
    println!("== 1. Noodle DM (real text, table only) ==");
    show(NOODLE);
    println!("== 2. Typed weight \"200g noodles\" (table only) ==");
    show(TYPED);
    println!("== 3. Receipt: insight::mask_cards ==");
    let text = RECEIPT.join("\n");
    println!("{}", insight::mask_cards(&text));
    println!("== 3b. Receipt: redact::sensitive ==");
    println!("{}", redact::sensitive(&text));
}
RS

# Driver for this head: the real pip::handle, the stub reader, and the same receipt masking.
cat > "$ROOT/demo130.rs" <<'RS'
//! After: this head. Each DM goes through pip::handle; the reader is the /bin/sh stub (PIP_MODEL) that prints the
//! fixture reply the driver copies into model-out first. Nothing here touches Discord or a token.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use discord_inbox::{insight, redact};
use std::fs;
use std::path::{Path, PathBuf};

const USER: u64 = 100000000000000001;
const CH: u64 = 5001;
const DAY: &str = "2026-10-28T12:00:00-04:00";
const NOODLE: &str = "100 grnas of noodle 100 grams of chicken";
const TYPED: &str = "200g noodles";
const RECEIPT: [&str; 8] = [
    "CORNER MART  store 0042",
    "Oat milk 1 L      4.99",
    "Bananas x6       12.50",
    "Total EUR     1.234,50",
    "Card 4111 1111 1111 1111",
    "Card 4111-1111-1111-1111",
    "Card 4111/1111/1111/1111",
    "Card 4111\u{200B}1111\u{200B}1111\u{200B}1111",
];

/// One DM. No photos: the reader gets the text only.
struct Msg {
    id: u64,
    text: &'static str,
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        USER
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        CH
    }
    fn received(&self) -> String {
        DAY.to_string()
    }
    fn content(&self) -> String {
        self.text.to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        Vec::new()
    }
    fn live(&self) -> bool {
        true
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Err("no attachments in this demo".to_string())
    }
    async fn react(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    async fn unreact(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        Vec::new()
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        for line in text.lines() {
            println!("    {line}");
        }
        Ok(())
    }
}

/// Puts a fixture reply where the stub will print it, then sends the DM through the real handler.
async fn send(allow: &allowlist::Allowlist, cfg: &Config, fixtures: &Path, reply: &Path, id: u64, text: &'static str, fixture: &str) {
    fs::copy(fixtures.join(fixture), reply).expect("copy the fixture reply");
    println!("> {text}   (reader reply: fixtures/{fixture})");
    let m = Msg { id, text };
    match pip::handle(allow, cfg, &m).await {
        Outcome::Saved => {}
        other => println!("  outcome for {id}: {other:?}"),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let (fixtures, reply) = (&args[0], &args[1]);
    let cfg = Config::from_env().expect("set PIP_NUTRITION_DIR and PIP_MODEL (an absolute path)");
    let allow = allowlist::parse(&format!("{USER}\n")).expect("one id");

    println!("== 1. Noodle DM (real text, reader reply from fixture) ==");
    send(&allow, &cfg, fixtures, reply, 9501, NOODLE, "noodle.json").await;
    println!("== 2. Typed weight \"200g noodles\" (reply leaves out grams) ==");
    send(&allow, &cfg, fixtures, reply, 9502, TYPED, "typed.json").await;

    let day = daylog::day_key(DAY);
    let note = daylog::note_path(&cfg.nutrition_dir, day);
    println!("== Day note: meal lines and macros, target {} kcal ==", cfg.kcal_target);
    for line in fs::read_to_string(&note).unwrap_or_default().lines().filter(|l| l.starts_with("- ")) {
        println!("  {line}");
    }
    println!("  fiber: not tracked on this head (no field in the reader or the day note); 30 g target not shown");

    let text = RECEIPT.join("\n");
    println!("== 3. Receipt: insight::mask_cards ==");
    println!("{}", insight::mask_cards(&text));
    println!("== 3b. Receipt: redact::sensitive ==");
    println!("{}", redact::sensitive(&text));
}
RS

# Before: origin/main, extracted to a fixed cache folder so cargo reuses the build between runs.
BEFORE=$(git -C "$REPO" rev-parse --short origin/main)
rm -rf "$CACHE/before" && mkdir -p "$CACHE/before"
git -C "$REPO" archive origin/main | tar -x -C "$CACHE/before"
mkdir -p "$CACHE/before/examples" && cp "$ROOT/demo130_before.rs" "$CACHE/before/examples/"
# After: this head (committed tree), extracted the same way.
HEAD_SHA=$(git -C "$REPO" rev-parse --short HEAD)
rm -rf "$CACHE/after" && mkdir -p "$CACHE/after"
git -C "$REPO" archive HEAD | tar -x -C "$CACHE/after"
mkdir -p "$CACHE/after/examples" && cp "$ROOT/demo130.rs" "$CACHE/after/examples/"
# Touch the sources so cargo rebuilds the library at each extraction (the shared target dir holds both).
find "$CACHE/before/src" "$CACHE/after/src" -name '*.rs' -exec touch {} +

echo "-- BEFORE: origin/main $BEFORE (table only) --"
(cd "$CACHE/before" && cargo run -q --release --example demo130_before)
echo
echo "-- AFTER: this head $HEAD_SHA (meal reader on a stub) --"
export PIP_NUTRITION_DIR="$ROOT/Private/Nutrition"
export PIP_MODEL="$ROOT/stub.sh"
export DISCORD_INBOX_INSIGHT=""
(cd "$CACHE/after" && cargo run -q --release --example demo130 -- "$ROOT/fixtures" "$ROOT/model-out")
echo
echo "Scratch folders are removed on exit. Nothing was sent to a model or a paid key."
