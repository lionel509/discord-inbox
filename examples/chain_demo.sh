#!/bin/bash
# Demos the chain (ScaleDown first, then Haiku). No spend: claude and curl are stubs. Usage: sh examples/chain_demo.sh (from repo root)
set -uo pipefail
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/worktrees/discord-inbox/.target}"
cargo build -q --release --example chain_demo || exit 1
D="$CARGO_TARGET_DIR/release/examples/chain_demo"
S=$(mktemp -d); trap 'rm -rf "$S"' EXIT
printf '#!/bin/sh\ncat >/dev/null\necho "claude-stub: haiku called" >&2\nif [ -n "$HAIKU_FAIL" ]; then exit 1; fi\nprintf "receipt: yes\\nmerchant: Hua Lian\\ntotal: 9.70\\ncategory: groceries\\n"\n' >"$S/claude"
printf '#!/bin/sh\ncat >/dev/null\necho "curl-stub: scaledown called" >&2\nif [ -n "$CURL_FAIL" ]; then exit 22; fi\nprintf "%%s" "{\\"choices\\":[{\\"message\\":{\\"content\\":\\"{\\\\\\"entities\\\\\\":[{\\\\\\"text\\\\\\":\\\\\\"Hua Lian\\\\\\",\\\\\\"type\\\\\\":\\\\\\"merchant\\\\\\",\\\\\\"confidence\\\\\\":0.9},{\\\\\\"text\\\\\\":\\\\\\"9.70\\\\\\",\\\\\\"type\\\\\\":\\\\\\"total\\\\\\",\\\\\\"confidence\\\\\\":0.9}]}\\"}}]}"\n' >"$S/curl"
chmod +x "$S/claude" "$S/curl"; echo sk-stub >"$S/key"
export DISCORD_INBOX_CLAUDE="$S/claude" DISCORD_INBOX_CURL="$S/curl" DISCORD_INBOX_OPENROUTER_KEY="$S/key"
OCR="華聯超市 總計 9.70"
echo "== which model each DISCORD_INBOX_INSIGHT value picks =="
echo "unset         -> $(env -u DISCORD_INBOX_INSIGHT "$D" model)   (chain: ScaleDown, then Haiku)"
echo "empty         -> $(DISCORD_INBOX_INSIGHT= "$D" model)"
echo "other command -> $(DISCORD_INBOX_INSIGHT='/bin/echo hi' "$D" model)"
echo "== chain, key file present, ScaleDown answers: Haiku is not called =="
echo "$OCR" | env -u DISCORD_INBOX_INSIGHT "$D" ask
echo "== chain, ScaleDown fails (curl exit 22): Haiku answers =="
echo "$OCR" | CURL_FAIL=1 env -u DISCORD_INBOX_INSIGHT "$D" ask
echo "== chain, no key file: Haiku answers, ScaleDown is not called =="
echo "$OCR" | DISCORD_INBOX_OPENROUTER_KEY="$S/nokey" env -u DISCORD_INBOX_INSIGHT "$D" ask
echo "== chain, no key file and Haiku fails (exit 1): no verdict =="
echo "$OCR" | HAIKU_FAIL=1 DISCORD_INBOX_OPENROUTER_KEY="$S/nokey" env -u DISCORD_INBOX_INSIGHT "$D" ask
