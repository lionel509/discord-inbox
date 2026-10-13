#!/bin/sh
# Offline demo: allowlist gate and Inbox writer, then the refuse-to-start check.
# No Discord, no token. Run from the repo root: sh examples/demo.sh
set -e
cd "$(dirname "$0")/.."
# Step 2 unsets HOME, so pin cargo's homes first or cargo loses its toolchain and registry.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
cargo build -q --bin discord-inbox --example demo
TMP=$(mktemp -d "${TMPDIR:-/tmp}/discord-inbox-run.XXXXXX")
# Mask the temp path with awk index(): sed would break on a delimiter character in TMPDIR.
mask() { T="$TMP" awk '{ o = ""; t = ENVIRON["T"]; while ((i = index($0, t)) > 0) { o = o substr($0, 1, i - 1) "<tmp>"; $0 = substr($0, i + length(t)) } print o $0 }' "$1"; }
trap 'rm -rf "$TMP"' EXIT

echo "== 1. allowlist set: main and alt accepted, third ID ignored =="
# Keep the demo's own status: a pipe would report sed's status and hide a failure.
set +e
cargo run -q --example demo > "$TMP/demo.txt" 2>&1
DEMO_RC=$?
set -e
mask "$TMP/demo.txt"
if [ "$DEMO_RC" -ne 0 ]; then
  echo "allowlist demo failed: exit $DEMO_RC" >&2
  exit "$DEMO_RC"
fi

echo
echo "== 2. allowlist unset: bot refuses to start =="
set +e
env -u DISCORD_INBOX_ALLOWED_IDS_FILE HOME="$TMP/home" DISCORD_INBOX_DIR="$TMP/Inbox" \
  cargo run -q --bin discord-inbox > "$TMP/refuse.txt" 2>&1
RC=$?
set -e
mask "$TMP/refuse.txt"
echo "exit code: $RC"
if [ "$RC" -ne 1 ]; then
  echo "step 2 did not refuse to start with exit 1" >&2
  exit 1
fi
