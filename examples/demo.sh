#!/bin/sh
# Offline demo: allowlist gate and Inbox writer, then the refuse-to-start check.
# No Discord, no token. Run from the repo root: sh examples/demo.sh
set -e
cd "$(dirname "$0")/.."
cargo build -q --bin discord-inbox --example demo
TMP=$(mktemp -d "${TMPDIR:-/tmp}/discord-inbox-run.XXXXXX")
trap 'rm -rf "$TMP"' EXIT

echo "== 1. allowlist set: main and alt accepted, third ID ignored =="
cargo run -q --example demo 2>&1 | sed "s#$TMP#<tmp>#g"

echo
echo "== 2. allowlist unset: bot refuses to start =="
set +e
env -u DISCORD_INBOX_ALLOWED_IDS_FILE HOME="$TMP/home" DISCORD_INBOX_DIR="$TMP/Inbox" \
  target/debug/discord-inbox > "$TMP/refuse.txt" 2>&1
RC=$?
sed "s#$TMP#<tmp>#g" "$TMP/refuse.txt"
echo "exit code: $RC"
