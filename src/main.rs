use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::dump;
use discord_inbox::chat::{self, OpenRouter};
use discord_inbox::ingest::{catch_up_with, ingest_with, Filer, History, Inbound, PAGE};
use discord_inbox::vaults::Vaults;
use serenity::all::{
    ActivityData, ChannelId, Client, Context, EventHandler, GatewayIntents, GetMessages, Http,
    Message, MessageId, OnlineStatus, ReactionType, Ready, UserId,
};
use serenity::async_trait;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval_at, Instant};

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
        (*self.msg.timestamp)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    }
    fn content(&self) -> String {
        self.msg.content.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.msg
            .attachments
            .iter()
            .map(|a| a.filename.clone())
            .collect()
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.msg.attachments[i]
            .download()
            .await
            .map_err(|e| e.to_string())
    }
    async fn react(&self, emoji: &str) -> Result<(), String> {
        self.msg
            .react(&self.http, ReactionType::Unicode(emoji.to_string()))
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    async fn unreact(&self, emoji: &str) -> Result<(), String> {
        self.msg
            .delete_reaction(&self.http, None, ReactionType::Unicode(emoji.to_string()))
            .await
            .map_err(|e| e.to_string())
    }
    fn own_reactions(&self) -> Vec<String> {
        self.msg
            .reactions
            .iter()
            .filter(|r| r.me)
            .filter_map(|r| match &r.reaction_type {
                ReactionType::Unicode(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        self.msg
            .channel_id
            .say(&self.http, text)
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
    filer: Filer,
}

/// Bot presence rotation, status always Do Not Disturb. Changed every `PRESENCE_INTERVAL`.
const PRESENCE_ROTATION: [(PresenceKind, &str); 4] = [
    (PresenceKind::Playing, "with a ball of yarn 🧶"),
    (PresenceKind::Watching, "the rain ⛈️"),
    (PresenceKind::Listening, "thunder"),
    (PresenceKind::Custom, "napping in a sunbeam ☀️"),
];
const PRESENCE_INTERVAL: Duration = Duration::from_secs(15 * 60);
static PRESENCE_LOOP_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
enum PresenceKind {
    Playing,
    Watching,
    Listening,
    Custom,
}

fn presence_activity(index: usize) -> ActivityData {
    let (kind, text) = PRESENCE_ROTATION[index];
    match kind {
        PresenceKind::Playing => ActivityData::playing(text),
        PresenceKind::Watching => ActivityData::watching(text),
        PresenceKind::Listening => ActivityData::listening(text),
        PresenceKind::Custom => ActivityData::custom(text),
    }
}

fn next_presence(index: usize) -> usize {
    (index + 1) % PRESENCE_ROTATION.len()
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _: Ready) {
        eprintln!("discord-inbox: connected");
        ctx.set_presence(Some(presence_activity(0)), OnlineStatus::DoNotDisturb);
        // Reconnects call `ready` again; only the first call starts the rotation.
        if PRESENCE_LOOP_STARTED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let mut tick = interval_at(Instant::now() + PRESENCE_INTERVAL, PRESENCE_INTERVAL);
                let mut index = 0;
                loop {
                    tick.tick().await;
                    index = next_presence(index);
                    ctx.set_presence(Some(presence_activity(index)), OnlineStatus::DoNotDisturb);
                }
            });
        }
        catch_up_with(
            &self.allow,
            &self.inbox,
            &Live {
                http: ctx.http.clone(),
            },
            &self.filer,
        )
        .await;
    }

    async fn message(&self, ctx: Context, msg: Message) {
        let _ = ingest_with(
            &self.allow,
            &self.inbox,
            &LiveMsg {
                http: ctx.http.clone(),
                msg,
            },
            &self.filer,
        )
        .await;
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
    // An empty or relative path would write notes relative to the working directory.
    if !inbox.is_absolute() {
        return fail(format!(
            "inbox dir {inbox:?} is not an absolute path; refusing to start"
        ));
    }
    if let Err(e) = fs::create_dir_all(&inbox) {
        return fail(format!("inbox {} not creatable: {e}", inbox.display()));
    }

    // Single instance: a second process would race this one on the same Inbox. The kernel drops
    // the lock when the process exits, even on kill -9, so a crash never blocks a restart.
    // Held as `_lock` until main returns, which is the whole run.
    let lock_path = inbox.join(".lock");
    let _lock = match fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => match f.try_lock() {
            Ok(()) => f,
            Err(fs::TryLockError::WouldBlock) => {
                return fail(format!(
                    "already running: {} is held by another instance",
                    lock_path.display()
                ))
            }
            Err(fs::TryLockError::Error(e)) => {
                return fail(format!("lock {} failed: {e}", lock_path.display()))
            }
        },
        Err(e) => return fail(format!("lock {} not openable: {e}", lock_path.display())),
    };

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

    let filer = match OpenRouter::from_key_file(&chat::key_path()) {
        Some(c) => Filer {
            chat: Arc::new(c),
            vaults: Vaults::new(Vaults::default_root()),
        },
        None => {
            eprintln!("discord-inbox: no openrouter key, chat replies off");
            Filer::off()
        }
    };

    // 3. DMs only: no guild intents, no message content intent.
    let mut client = match Client::builder(&token, GatewayIntents::DIRECT_MESSAGES)
        .event_handler(Handler {
            allow,
            inbox,
            filer,
        })
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

#[cfg(test)]
mod presence_tests {
    use super::*;

    #[test]
    fn presence_rotation_is_nonempty_and_wraps() {
        let n = PRESENCE_ROTATION.len();
        assert!(n > 0);
        assert_eq!(next_presence(0), 1 % n);
        assert_eq!(next_presence(n - 1), 0);
    }
}
