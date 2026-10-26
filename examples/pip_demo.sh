#!/bin/sh
# Pip demo: the real handler and parser on typed input, then a replay of DMs sent while the laptop was off.
# Scratch folders only (Private/Recipes and Private/Nutrition under a temp dir); no Discord, no token.
# Run from the repo root: sh examples/pip_demo.sh
set -e
cd "$(dirname "$0")/.."
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/worktrees/discord-inbox/.target}"
ROOT=$(mktemp -d "${TMPDIR:-/tmp}/pip-demo.XXXXXX")
trap 'rm -rf "$ROOT"' EXIT
mkdir -p "$ROOT/Private/Recipes" "$ROOT/Private/Nutrition"

# Recipe fixtures: a gram table and a Recipe/Batch table. The demo only reads them.
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
cat > "$ROOT/Private/Recipes/Cucumber Side.md" <<'NOTE'
# Cucumber Side

| Ingredient | Recipe | Batch 1 |
|---|---|---|
| Cucumber | 300 | 900 |
| Sesame oil | 5 | — |
NOTE

export PIP_NUTRITION_DIR="$ROOT/Private/Nutrition"
export PIP_RECIPES_DIR="$ROOT/Private/Recipes"

echo "Scratch folders: $ROOT/Private (removed on exit)"
echo "Recipes before: $(ls "$PIP_RECIPES_DIR" | wc -l | tr -d ' ') notes"
cargo run -q --release --example pip_demo
echo "Recipes after: $(ls "$PIP_RECIPES_DIR" | wc -l | tr -d ' ') notes (unchanged: Pip never writes there)"
