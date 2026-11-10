#!/bin/sh
# #146 demo: a label photo saves a food note, then "24 wontons", "wontons" and "wontons 200g" log from it.
# Scratch folders only. Stub reader by default; set PIP_DEMO_MODEL=<helper> and PIP_DEMO_PHOTO=<label.jpg> for the real one.
set -e
cd "$(dirname "$0")/.."
root="${PIP_DEMO_ROOT:-$(mktemp -d "${TMPDIR:-/tmp}/pip-146-demo.XXXXXX")}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/worktrees/discord-inbox/.target}"
cargo run -q --example pip_foods_demo -- "$root"
echo "scratch: $root"
