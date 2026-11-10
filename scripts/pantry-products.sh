#!/bin/sh
# Pantry products extractor (#159): Haiku on the Claude subscription, direct to Anthropic (never the local router, so no
# failover to a metered model). stdin: the prompt. stdout: the reply lines. Tools off, no working directory.
cd /tmp || exit 1
exec /usr/bin/env -u ANTHROPIC_BASE_URL -u ANTHROPIC_API_KEY -u ANTHROPIC_AUTH_TOKEN -u CLAUDE_CODE_USE_BEDROCK -u CLAUDE_CODE_USE_VERTEX "${DISCORD_INBOX_CLAUDE:-$HOME/.local/bin/claude}" -p --model haiku \
  --strict-mcp-config --tools "" --no-session-persistence --output-format text
