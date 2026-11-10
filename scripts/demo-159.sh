#!/bin/sh
# #159 demo: bought products and recipe-note kcal through the real meal reader, on made-up scratch folders.
# Usage: scripts/demo-159.sh <path to the sandwich photo>. The photo is copied into a scratch folder, never the repo.
set -e
if [ -z "$1" ] || [ ! -f "$1" ]; then
    echo "usage: scripts/demo-159.sh <photo>" >&2
    exit 2
fi
photo="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/worktrees/discord-inbox/.target}"
cargo run -q --example pip_pantry_demo -- "$photo"
