# discord-inbox

A Discord bot that takes DMs from two allowlisted accounts and dumps each one (text and attachments) into `~/Documents/Inbox/`. Everyone else is ignored silently: no reply, no reaction, no file.

## Setup

1. Allowlist: one Discord user ID per line, outside the repo.
   `mkdir -p ~/.config/discord-inbox && printf '%s\n' <main-id> <alt-id> > ~/.config/discord-inbox/allowed_ids`
   The bot refuses to start if the file is missing or lists no IDs.
2. Token: `security add-generic-password -s discord-inbox -a discord-inbox -w`
3. Discord developer portal: bot token, Message Content Intent off. The bot uses DM intents only.

Env overrides: `DISCORD_INBOX_ALLOWED_IDS_FILE`, `DISCORD_INBOX_DIR`.

## Run and test

- `cargo run --release`: connects and dumps. On start it first retries any DM in `Inbox/.retry`, then catches up on each DM channel from its own cursor (`Inbox/.cursor-<channel>`).
- `cargo test`: unit tests, plus integration tests that check refuse-to-start.
- `sh examples/demo.sh`: offline demo of the allowlist gate, the Inbox writer, the retry list and the refuse-to-start check. No Discord, no token.

## When a download fails

The note is written anyway. The failed attachment is listed as `sha256: pending`, the message is recorded in `Inbox/.retry`, and it is retried on every start until it completes. The ✅ appears only once every attachment is saved. Nothing is dropped.

## First start

On the first start (or if a channel's cursor file is lost), only the newest 100 messages per DM channel are fetched; older history is not backfilled.

Builds on macOS only (Keychain via `security-framework`).
