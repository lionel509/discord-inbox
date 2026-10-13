use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::dump::{self, Incoming};
use serenity::all::{
    Client, Context, EventHandler, GatewayIntents, GetMessages, Message, MessageId, Ready, UserId,
};
use serenity::async_trait;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

const KEYCHAIN_SERVICE: &str = "discord-inbox";
const KEYCHAIN_ACCOUNT: &str = "discord-inbox";
const PAGE: u64 = 100;

struct Handler {
    allow: Allowlist,
    inbox: PathBuf,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _: Ready) {
        eprintln!("discord-inbox: connected");
        catch_up(&ctx, self).await;
    }

    async fn message(&self, ctx: Context, msg: Message) {
        ingest(&ctx, self, &msg).await;
    }
}

/// The only place that writes, advances the cursor or reacts. Both the live
/// event and the startup catch-up go through here.
async fn ingest(ctx: &Context, h: &Handler, msg: &Message) {
    // Gate first: a sender not on the allowlist gets no download, write, reaction or log line.
    if !h.allow.contains(msg.author.id.get()) {
        return;
    }

    // ponytail: a failed download or write leaves the cursor where it was, so the
    // message is retried on the next start. A later message that succeeds advances past it.
    let mut files = Vec::new();
    for a in &msg.attachments {
        match a.download().await {
            Ok(bytes) => files.push((a.filename.clone(), bytes)),
            Err(e) => {
                eprintln!(
                    "discord-inbox: attachment {} of {} failed: {e}",
                    a.filename, msg.id
                );
                return;
            }
        }
    }

    let received = (*msg.timestamp).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let incoming = Incoming {
        msg_id: msg.id.get(),
        author_id: msg.author.id.get(),
        received,
        content: msg.content.clone(),
        attachments: files,
    };
    match dump::admit_and_write(&h.allow, &h.inbox, &incoming) {
        Ok(Some(_)) => {}
        Ok(None) => return,
        Err(e) => {
            eprintln!("discord-inbox: write {} failed: {e}", msg.id);
            return;
        }
    }
    if let Err(e) = dump::write_last_id(&h.inbox, msg.id.get()) {
        eprintln!("discord-inbox: cursor write failed: {e}");
    }
    if let Err(e) = msg.react(&ctx.http, '✅').await {
        eprintln!("discord-inbox: react on {} failed: {e}", msg.id);
    }
}

/// Pages each allowlisted DM channel forward from the saved cursor.
async fn catch_up(ctx: &Context, h: &Handler) {
    // Snapshot once: a channel paged after another channel moved the cursor would skip its older DMs.
    let start = dump::read_last_id(&h.inbox);
    for id in h.allow.ids() {
        let dm = match UserId::new(id).create_dm_channel(&ctx.http).await {
            Ok(ch) => ch,
            Err(e) => {
                eprintln!("discord-inbox: dm channel failed: {e}");
                continue;
            }
        };
        // Local cursor so paging always moves forward, even if every message on a page fails.
        let mut after = start;
        loop {
            let mut req = GetMessages::new().limit(PAGE as u8);
            if let Some(last) = after {
                req = req.after(MessageId::new(last));
            }
            let mut page: Vec<Message> = match dm.id.messages(&ctx.http, req).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("discord-inbox: history failed: {e}");
                    break;
                }
            };
            page.sort_by_key(|m| m.id);
            for m in &page {
                ingest(ctx, h, m).await;
            }
            let Some(max) = page.last().map(|m| m.id.get()) else {
                break;
            };
            if (page.len() as u64) < PAGE || after.is_some_and(|a| max <= a) {
                break;
            }
            after = Some(max);
        }
    }
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("discord-inbox: {msg}");
    ExitCode::FAILURE
}

#[tokio::main]
async fn main() -> ExitCode {
    // 1. Allowlist first: an unset or empty list refuses to start before anything else runs.
    let allow = match Allowlist::load(&allowlist::default_path()) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    let inbox = dump::inbox_dir();
    if let Err(e) = fs::create_dir_all(&inbox) {
        return fail(format!("inbox {} not creatable: {e}", inbox.display()));
    }

    // 2. Token from the login Keychain. Never printed.
    let token = match security_framework::passwords::get_generic_password(
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT,
    ) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s.trim().to_string(),
            Err(_) => return fail("keychain item discord-inbox is not UTF-8"),
        },
        Err(e) => return fail(format!("keychain item discord-inbox not readable: {e}")),
    };

    // 3. DMs only: no guild intents, no message content intent.
    let mut client = match Client::builder(&token, GatewayIntents::DIRECT_MESSAGES)
        .event_handler(Handler { allow, inbox })
        .await
    {
        Ok(c) => c,
        Err(e) => return fail(format!("client build failed: {e}")),
    };
    if let Err(e) = client.start().await {
        return fail(format!("gateway stopped: {e}"));
    }
    ExitCode::SUCCESS
}
