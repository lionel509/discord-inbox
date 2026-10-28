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

# DEMO_TODAY pins the date (YYYY-MM-DD) for a repeatable run; the default is today.
TODAY="${DEMO_TODAY:-$(date +%Y-%m-%d)}"
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
    | grep -E ' - (Lectures|Exams|Work|Office Hours)$|^SBU - |^Career$|^Study$' > "$SCRATCH/calendars" || true
  if [ -s "$SCRATCH/calendars" ]; then
    echo "calendars: built from Calendar.app (course calendars only)"
  else
    # Reads come from the fixture database, so the made-up CHEM 103 calendars are the only ones shown.
    export DISCORD_INBOX_CALENDAR_DB="$REPO/tests/fixtures/calendar.sqlitedb"
    printf '%s\n' 'CHEM 103 - Lectures' 'CHEM 103 - Exams' 'Career' 'Study' > "$SCRATCH/calendars"
    echo "calendars: unavailable, using the FIXTURE calendar (made-up CHEM 103)"
  fi
fi

export DISCORD_INBOX_VAULTS_DIR="$SCRATCH/vaults"
export DISCORD_INBOX_CALENDARS_FILE="$SCRATCH/calendars"
export DISCORD_INBOX_DIR="$SCRATCH/inbox"
export DISCORD_INBOX_TODAY="$TODAY"

cd "$REPO"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/worktrees/discord-inbox/.target}"
BIN_ARGS="--quiet --bin stormy-daily --"
echo "== morning (printed, not sent) =="
cargo run --release $BIN_ARGS morning
echo
echo "== night (printed, not sent) =="
cargo run --release $BIN_ARGS night
echo
# Read-only cross-check: the dates the database read prints for the - Exams calendars, against Calendar.app itself.
echo "== cross-check: AppleScript read of the - Exams calendars, $TODAY to +3 days =="
cat > "$SCRATCH/xcheck.applescript" <<'EOF'
on run argv
  set d0 to current date
  set day of d0 to 1
  set year of d0 to (item 1 of argv) as integer
  set month of d0 to (item 2 of argv) as integer
  set day of d0 to (item 3 of argv) as integer
  set time of d0 to 0
  set d1 to d0 + 4 * days
  set out to {}
  tell application "Calendar"
    repeat with i from 4 to count of argv
      set calName to item i of argv
      repeat with ev in (every event of calendar calName whose start date >= d0 and start date < d1)
        set end of out to (calName & " | " & ((start date of ev) as string) & " | " & (summary of ev))
      end repeat
    end repeat
  end tell
  set AppleScript's text item delimiters to linefeed
  return out as text
end run
EOF
grep -- ' - Exams$' "$SCRATCH/calendars" | tr '\n' '\0' | xargs -0 /usr/bin/osascript "$SCRATCH/xcheck.applescript" $(echo "$TODAY" | tr '-' ' ') || echo "cross-check failed"
echo
echo "== simulated night reply =="
cargo run --release $BIN_ARGS reply "did the lab report, still need to study for the ESE quiz, tomorrow i have to email the TA"
echo
echo "== to-do folder diff (scratch copy vs real) =="
diff -ru "$REAL_TODO" "$SCRATCH/vaults/! Todo" || true
