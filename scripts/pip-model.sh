#!/bin/sh
# Pip's meal reader: Haiku on the Claude subscription, direct to Anthropic (never the local router, so no
# failover to a metered model). stdin: the prompt. args: saved photo paths. stdout: JSON. Swap the model here.
dir=""; [ -n "$1" ] && dir=$(dirname "$1")
exec /usr/bin/env -u ANTHROPIC_BASE_URL -u ANTHROPIC_API_KEY -u ANTHROPIC_AUTH_TOKEN -u CLAUDE_CODE_USE_BEDROCK -u CLAUDE_CODE_USE_VERTEX "${DISCORD_INBOX_CLAUDE:-$HOME/.local/bin/claude}" -p --model haiku \
  --strict-mcp-config --tools Read --allowedTools Read ${dir:+--add-dir "$dir"} --no-session-persistence --output-format text
