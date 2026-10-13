use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::dump;
use discord_inbox::ingest::{catch_up, ingest, History, Inbound, PAGE};
use serenity::all::{
    ChannelId, Client, Context, EventHandler, GatewayIntents, GetMessages, Http, Message,
    MessageId, Ready, UserId,
};
use serenity::async_trait;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

const KEYCHAIN_SERVICE: &str = "discord-inbox";
const KEYCHAIN_ACCOUNT: &str = "discord-inbox";

/// One serenity message, adapted to the lib's `Inbound`.
struct LiveMsg {
    http: Arc<Http>,
    msg: Message,
}

impl Inbound for LiveMsg {
    fn author_id(&self) -> u64 {
        self.msg.author.id.get()
    }
    fn msg_id(&self) -> u64 {
        self.msg.id.get()
    }
    fn channel_id(&self) -> u64 {
        self.msg.channel_id.get()
    }
    fn received(&self) -> String {
        (*self.msg.timestamp).format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }
    fn content(&self) -> String {
        self.msg.content.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.msg.attachments.iter().map(|a| a.filename.clone()).collect()
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.msg.attachments[i].download().await.map_err(|e| e.to_string())
    }
    async fn react(&self) -> Result<(), String> {
        self.msg
            .react(&self.http, '✅')
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// The Discord history calls, adapted to the lib's `History`.
struct Live {
    http: Arc<Http>,
}

impl History for Live {
    type Msg = LiveMsg;

    async fn dm_channel(&self, user_id: u64) -> Result<u64, String> {
        UserId::new(user_id)
            .create_dm_channel(&self.http)
            .await
            .map(|c| c.id.get())
            .map_err(|e| e.to_string())
    }

    async fn page(&self, channel: u64, after: Option<u64>) -> Result<Vec<LiveMsg>, String> {
        let mut req = GetMessages::new().limit(PAGE as u8);
        if let Some(a) = after {
            req = req.after(MessageId::new(a));
        }
        let msgs = ChannelId::new(channel)
            .messages(&self.http, req)
            .await
            .map_err(|e| e.to_string())?;
        Ok(msgs
            .into_iter()
            .map(|msg| LiveMsg {
                http: self.http.clone(),
                msg,
            })
            .collect())
    }

    async fn fetch(&self, channel: u64, msg_id: u64) -> Result<LiveMsg, String> {
        let msg = ChannelId::new(channel)
            .message(&self.http, MessageId::new(msg_id))
            .await
            .map_err(|e| e.to_string())?;
        Ok(LiveMsg {
            http: self.http.clone(),
            msg,
        })
    }
}

struct Handler {
    allow: Allowlist,
    inbox: PathBuf,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _: Ready) {
        eprintln!("discord-inbox: connected");
        catch_up(&self.allow, &self.inbox, &Live { http: ctx.http.clone() }).await;
    }

    async fn message(&self, ctx: Context, msg: Message) {
        let _ = ingest(&self.allow, &self.inbox, &LiveMsg { http: ctx.http.clone(), msg }).await;
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
