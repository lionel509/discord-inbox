#!/bin/sh
# Stormy's small-task demo (#84): one message through the helper on a scratch copy of the finance vault. Never the real one.
# Run: sh examples/task_demo.sh "<message>" [--reset]
set -e
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="$HOME/worktrees/discord-inbox/.target"
D="$HOME/worktrees/discord-inbox/demo-root"
if [ "$2" = "--reset" ] || [ ! -d "$D/State Street/.git" ]; then
  rm -rf "$D"; mkdir -p "$D/Private"
  cp -R "$HOME/Documents/State Street" "$D/"
  # Drop the hand booking of Paco's $25.40 (d400d92) so the demo's booking is not a duplicate. Scratch copy only.
  git -C "$D/State Street" reset -q --hard d400d92~1
  cp "$HOME/Documents/CLAUDE.md" "$D/CLAUDE.md"
  echo secret > "$D/Private/note.md"
fi
curl -sf http://127.0.0.1:8787/v1/models >/dev/null || { echo "router down"; exit 2; }
REAL=$(git -C "$HOME/Documents/State Street" rev-parse --short HEAD)
DISCORD_INBOX_VAULTS_DIR="$D" cargo run -q --example task_demo -- "$1"
echo "scratch: $(git -C "$D/State Street" log -1 --format='%h %s')"
echo "scratch status:"; git -C "$D/State Street" status --porcelain
echo "real vault still at $REAL: $(git -C "$HOME/Documents/State Street" rev-parse --short HEAD)"
