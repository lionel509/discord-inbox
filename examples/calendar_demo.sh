#!/bin/bash
# Demo for #95: the done-with-task check-off against the calendar stub. Real Calendar.app is never called. Usage: sh examples/calendar_demo.sh (from repo root)
set -uo pipefail
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/worktrees/discord-inbox/.target}"
cargo build -q --example calendar_demo || exit 1
D="$CARGO_TARGET_DIR/debug/examples/calendar_demo"
S=$(mktemp -d); trap 'rm -rf "$S"' EXIT
printf 'MATH 101 - Work\nPHYS 102 - Exams\n' >"$S/calendars"
export DISCORD_INBOX_CALENDARS_FILE="$S/calendars"
export DISCORD_INBOX_OSASCRIPT="$PWD/examples/calendar_stub.sh"
# Reads come from the fixture, so the "after" list will not show the stub's tick.
export DISCORD_INBOX_CALENDAR_DB="$PWD/tests/fixtures/calendar.sqlitedb"
export DISCORD_INBOX_CALENDAR_LOG="$S/stub.log"
export DISCORD_INBOX_CALENDAR_STATE="$S/state"
"$D"
echo "== argv the stub received (what osascript would have been given; tabs shown as |) =="
tr '\t' '|' <"$S/stub.log"
