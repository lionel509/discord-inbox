#!/bin/sh
# Stands in for osascript in tests and the demo (#95). Reads and ignores stdin; never touches a real calendar.
# Every call is appended to $DISCORD_INBOX_CALENDAR_LOG when set, so a test can see the exact argv.
cat >/dev/null
[ "$1" = "-" ] && shift
if [ -n "$DISCORD_INBOX_CALENDAR_LOG" ]; then
  IFS=$(printf '\t'); echo "$*" >>"$DISCORD_INBOX_CALENDAR_LOG"; unset IFS
fi
if [ -n "$DISCORD_INBOX_CALENDAR_STUB_FAIL" ]; then exit 1; fi
# uid, calendar, date, summary. Made-up courses; the Home row is a personal calendar nothing should touch.
rows() {
  printf 'u-math-hw3\tMATH 101 - Work\t2026-10-20\tHW 3 due\n'
  printf 'u-math-hw2\tMATH 101 - Work\t2026-10-06\t✅ HW 2 due\n'
  printf 'u-phys-mid\tPHYS 102 - Exams\t2026-10-21\tMidterm 1\n'
  printf 'u-phys-lab\tPHYS 102 - Exams\t2026-10-28\tLab 2 report due\n'
  printf 'u-home-1\tHome\t2026-10-19\tPersonal thing\n'
}
case "$1" in
  list)
    shift
    set_=$(printf '\t%s' "$@")"\t"
    rows | awk -F'\t' -v set="$set_" 'index(set, "\t" $2 "\t")'
    ;;
  check)
    line=$(rows | awk -F'\t' -v u="$3" -v c="$2" '$1 == u && $2 == c')
    [ -z "$line" ] && exit 1
    summary=$(printf '%s' "$line" | awk -F'\t' '{ print $4 }')
    case "$summary" in "✅"*) ;; *) summary="✅ $summary" ;; esac
    printf '%s\n' "$summary"
    ;;
  *) exit 1 ;;
esac
