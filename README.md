# discord-inbox

A Discord bot that takes DMs from two allowlisted accounts and dumps each one (text and attachments) into `~/Documents/Inbox/`. Everyone else is ignored silently: no reply, no reaction, no file.

Image attachments (png, jpg, heic, webp, gif, tiff, bmp) are read with Apple Vision on this Mac, and the text goes into the note under `## OCR <file>`. Nothing leaves the machine. If OCR fails, that section is simply missing; the note, the ✅ and the retry don't change. `DISCORD_INBOX_OCR` overrides the helper path.

A receipt DM gets a reply in the same DM, sent after the ✅: the OCR text with card numbers masked to the last four digits, then a one-line verdict from a local model (`ollama run qwen3.5:4b`, receipt / merchant / total / category, capped at 60 s; nothing leaves the Mac), then, when `DISCORD_INBOX_BUDGET` is set, the budget left for that month before this receipt. The bot never writes the budget note, and any part that is missing is simply left out; the note, the ✅ and the retry don't change.

## Setup

1. Allowlist: one Discord user ID per line, outside the repo.
   `mkdir -p ~/.config/discord-inbox && printf '%s\n' <main-id> <alt-id> > ~/.config/discord-inbox/allowed_ids`
   The bot refuses to start if the file is missing or lists no IDs.
2. Token: `security add-generic-password -s discord-inbox -a discord-inbox -w`
3. Discord developer portal: bot token, Message Content Intent off. The bot uses DM intents only.

Env overrides: `DISCORD_INBOX_ALLOWED_IDS_FILE`, `DISCORD_INBOX_DIR`, `DISCORD_INBOX_INSIGHT` (the full model command, split on spaces; put a quoted part in `"..."` if it has a space; empty skips the model), `DISCORD_INBOX_BUDGET` (path template where `{month}` becomes the message's `YYYY-MM`; the note needs one line containing "budget" with a `$amount`, and spending lines `YYYY-MM-DD … $amount`).

## Run and test

- `cargo run --release`: connects and dumps. On start it first retries any DM in `Inbox/.retry`, then catches up on each DM channel from its own cursor (`Inbox/.cursor-<channel>`).
- `cargo test`: unit tests, plus integration tests that check refuse-to-start.
- `sh examples/demo.sh`: offline demo of the allowlist gate, the Inbox writer, the retry list and the refuse-to-start check. No Discord, no token.
- `DISCORD_INBOX_DIR=<scratch> cargo run --example ocr_demo -- <image>...`: writes a note per image through the real ingest path and prints it. Use a scratch dir, never your real Inbox.

## When a download fails

The note is written anyway. The failed attachment is listed as `sha256: pending`, the message is recorded in `Inbox/.retry`, and it is retried on every start until it completes. While it waits the message carries ⏳; the start that completes it swaps ⏳ for ✅. A message whose note could not be written at all gets ⚠️ and nothing on disk, so resend it; it is also retried from the cursor on the next start, and ⚠️ becomes ✅ if that works. A message shows one mark, except when Discord refuses to remove the old one: that failure is logged, the new mark is still added beside it, so ✅ always means the note is in the vault. If Discord refuses the new mark, the message can be left with none; that is logged too. A message already waiting in the retry list keeps ⏳ even if a later write fails, because its note is still on disk. Otherwise reactions are best-effort: a failed react is logged and changes nothing else. Nothing is dropped.

## First start

Until a DM channel has a cursor (`Inbox/.cursor-<channel>`), each start fetches only the newest 100 messages in it; older history is not backfilled.

Builds on macOS only (Keychain via `security-framework`).

OCR needs Xcode's `swiftc` at build time; without it the build warns and OCR is off.
