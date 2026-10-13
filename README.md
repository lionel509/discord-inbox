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

- `cargo run --release`: connects and dumps. On start it catches up on DMs since the last saved message.
- `cargo test`: unit tests, plus integration tests that check refuse-to-start.
- `sh examples/demo.sh`: offline demo of the allowlist gate, the Inbox writer and the refuse-to-start check. No Discord, no token.

Builds on macOS only (Keychain via `security-framework`).
