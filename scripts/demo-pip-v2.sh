#!/bin/sh
# Pip v2 demo: real DM lines through the parser, then a recipe, undo and fix through the real handler.
# Part 1 runs on origin/main (685e9de) too, so the same lines show before and after side by side.
# Everything runs in scratch folders under a temp dir; no Discord, no Keychain token, no real vault.
# Run from anywhere: sh scripts/demo-pip-v2.sh   (CARGO_TARGET_DIR optional; default ~/.cache/cargo-target)
set -e
cd "$(dirname "$0")/.."
REPO=$(pwd)
BEFORE=685e9de
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/cargo-target}"
ROOT=$(mktemp -d "${TMPDIR:-/tmp}/pip-demo-v2.XXXXXX")
trap 'rm -rf "$ROOT"' EXIT
case "$ROOT" in
  "$HOME"/Documents*|"$HOME"/documents*) echo "refusing: scratch folder under ~/Documents" >&2; exit 1 ;;
esac
mkdir -p "$ROOT/Private/Recipes" "$ROOT/Private/Nutrition"

# Recipe fixture for part 2 (the scratch copy only; Pip never writes recipes).
cat > "$ROOT/Private/Recipes/Salmon Bowl.md" <<'NOTE'
---
aliases: [salmon bowl]
---
# Salmon Bowl

## Per 100 g portion

| Ingredient | g |
|---|---|
| Salmon fillet, skinless | 100 |
| Dry rice | 92.5 |
| Cabbage | 100 |
| Neutral oil | 13.6 |
NOTE

echo "== Part 1: the same DM lines through the parser =="
echo
echo "-- BEFORE: origin/main $BEFORE --"
if git cat-file -e "$BEFORE^{commit}" 2>/dev/null; then
  mkdir -p "$ROOT/before"
  git archive "$BEFORE" | tar -x -C "$ROOT/before"
  cp "$REPO/examples/pip_lines_demo.rs" "$ROOT/before/examples/"
  (cd "$ROOT/before" && cargo run -q --release --example pip_lines_demo)
else
  echo "($BEFORE is not in this clone; skipping the before run)"
fi
echo
# The before build shares CARGO_TARGET_DIR and overwrites the example binaries under the same names, and
# cargo does not re-copy an example it thinks is fresh. Touching this branch's sources forces a rebuild
# and a fresh copy, so the AFTER and Part 2 runs can never execute origin/main's binary.
find "$REPO/src" "$REPO/examples" -name '*.rs' -exec touch {} +
echo "-- AFTER: this branch --"
cargo run -q --release --example pip_lines_demo
echo
echo "== Part 2: recipe, undo and fix through the real handler (scratch folders) =="
export PIP_NUTRITION_DIR="$ROOT/Private/Nutrition"
export PIP_RECIPES_DIR="$ROOT/Private/Recipes"
cargo run -q --release --example pip_flow_demo
echo
echo "Scratch folders are removed on exit; recipe notes untouched: $(ls "$PIP_RECIPES_DIR" | wc -l | tr -d ' ') file(s)."
