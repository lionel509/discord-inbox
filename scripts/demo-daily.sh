#!/bin/sh
# Dry run of the daily brief (#135) on real input: morning DM, night DM, one simulated night reply,
# then the to-do diff. Nothing is sent to Discord and the real to-do folder is only read.
set -eu

REPO=$(cd "$(dirname "$0")/.." && pwd)
REAL_VAULTS="${DISCORD_INBOX_VAULTS_DIR:-$HOME/Documents}"
REAL_TODO="$REAL_VAULTS/! Todo"
SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/discord-inbox-daily.XXXXXX")
trap 'rm -rf "$SCRATCH"' EXIT INT TERM

mkdir -p "$SCRATCH/vaults/! Todo" "$SCRATCH/inbox"
if [ -d "$REAL_TODO" ]; then
  cp -R "$REAL_TODO/." "$SCRATCH/vaults/! Todo/"
fi

TODAY=$(date +%Y-%m-%d)
if [ ! -f "$SCRATCH/vaults/! Todo/$TODAY.md" ]; then
  printf '%s\n' '---' 'tags:' '  - todo' '---' '- sample: finish the lab report' '- sample: email the TA' \
    > "$SCRATCH/vaults/! Todo/$TODAY.md"
  echo "todo: today's file is missing in the real folder, so the demo uses two sample lines (labelled sample)"
else
  echo "todo: using the real $TODAY.md copy"
fi

# Calendars: the real names if a calendars file exists, else the school calendars from Calendar.app.
# A bare Home, Work, Important or lionel name can never match this filter.
CAL_FILE="$HOME/.config/discord-inbox/calendars"
if [ -s "$CAL_FILE" ]; then
  cp "$CAL_FILE" "$SCRATCH/calendars"
  echo "calendars: copied from ~/.config/discord-inbox/calendars"
else
  osascript -e 'tell application "Calendar" to name of every calendar' 2>/dev/null \
    | tr ',' '\n' | sed 's/^ *//' \
    | grep -E ' - (Lectures|Exams|Work|Office Hours)$|^SBU - ' > "$SCRATCH/calendars" || true
  if [ -s "$SCRATCH/calendars" ]; then
    echo "calendars: built from Calendar.app (course calendars only)"
  else
    : > "$SCRATCH/calendars"
    export DISCORD_INBOX_OSASCRIPT="$REPO/examples/calendar_stub.sh"
    echo "calendars: unavailable (no Automation permission yet), using STUB calendar data"
  fi
fi

export DISCORD_INBOX_VAULTS_DIR="$SCRATCH/vaults"
export DISCORD_INBOX_CALENDARS_FILE="$SCRATCH/calendars"
export DISCORD_INBOX_DIR="$SCRATCH/inbox"

cd "$REPO"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/worktrees/discord-inbox/.target}"
BIN_ARGS="--quiet --bin stormy-daily --"
echo "== morning (printed, not sent) =="
cargo run --release $BIN_ARGS morning
echo
echo "== night (printed, not sent) =="
cargo run --release $BIN_ARGS night
echo
echo "== simulated night reply =="
cargo run --release $BIN_ARGS reply "did the lab report, still need to study for the ESE quiz, tomorrow i have to email the TA"
echo
echo "== to-do folder diff (scratch copy vs real) =="
diff -ru "$REAL_TODO" "$SCRATCH/vaults/! Todo" || true
