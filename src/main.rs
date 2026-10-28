use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::calendar::Cal;
use discord_inbox::daily::{self, Deps, Slot};
use discord_inbox::dump;
use discord_inbox::chat::{self, OpenRouter};
use discord_inbox::ingest::{catch_up_with, ingest_with, Filer};
use discord_inbox::live::{Live, LiveMsg};
use discord_inbox::todo;
use discord_inbox::vaults::Vaults;
use serenity::all::{
    ActivityData, Client, Context, EventHandler, GatewayIntents, Message, OnlineStatus, Ready,
    UserId,
};
use serenity::async_trait;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::{interval, interval_at, Instant, Interval, MissedTickBehavior};

const KEYCHAIN_SERVICE: &str = "discord-inbox";
const KEYCHAIN_ACCOUNT: &str = "discord-inbox";

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
/// The one live presence loop. Each `ready` replaces it, so the loop always holds the
/// Context from the most recent identify, and the old loop is aborted.
static PRESENCE_LOOP: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

fn install_presence_loop(handle: JoinHandle<()>) {
    let previous = PRESENCE_LOOP.lock().unwrap().replace(handle);
    if let Some(old) = previous {
        old.abort();
    }
}

/// The one live daily loop (#135), replaced on each `ready` like the presence loop.
static DAILY_LOOP: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

fn install_daily_loop(handle: JoinHandle<()>) {
    let previous = DAILY_LOOP.lock().unwrap().replace(handle);
    if let Some(old) = previous {
        old.abort();
    }
}

/// Ticks every `daily::TICK`; a late tick runs once, it does not burst.
fn daily_interval() -> Interval {
    let mut tick = interval(daily::TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    tick
}

/// Skip, not Burst: a stalled runtime must not fire every missed tick back to back.
fn presence_interval() -> Interval {
    let mut tick = interval_at(Instant::now() + PRESENCE_INTERVAL, PRESENCE_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tick
}

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
        // Every `ready` (including a re-identify) replaces the loop, so it never holds a dead shard's Context.
        let loop_ctx = ctx.clone();
        install_presence_loop(tokio::spawn(async move {
            let mut tick = presence_interval();
            let mut index = 0;
            loop {
                tick.tick().await;
                index = next_presence(index);
                loop_ctx.set_presence(Some(presence_activity(index)), OnlineStatus::DoNotDisturb);
            }
        }));
        // Daily brief (#135): one marker per slot per day, staged before the send, so a failed send is logged and not retried.
        let ids: Vec<u64> = self.allow.ids().collect();
        let inbox = self.inbox.clone();
        let http = ctx.http.clone();
        let chat = self.filer.chat.clone();
        let todo_dir = self.filer.vaults.root().join(todo::DIR);
        let times = daily::times_from_env();
        install_daily_loop(tokio::spawn(async move {
            let mut tick = daily_interval();
            loop {
                tick.tick().await;
                let now = chrono::Local::now().naive_local();
                let Some((slot, day)) = daily::due(&inbox, &times, now) else {
                    continue;
                };
                if let Err(e) = daily::stage(&inbox, slot, &day) {
                    eprintln!("discord-inbox: daily {slot:?} marker for {day} not written: {e}");
                    continue;
                }
                let deps = Deps {
                    todo_dir: todo_dir.clone(),
                    cal: Cal::from_env(),
                    chat: chat.clone(),
                };
                let today = now.date();
                let text = match tokio::task::spawn_blocking(move || daily::compose(&deps, slot, today)).await {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("discord-inbox: daily {slot:?} compose failed: {e}");
                        continue;
                    }
                };
                // The night reply is read against the list this DM numbers, so save it before the DM goes out.
                if matches!(slot, Slot::Night) {
                    if let Err(e) = daily::save_list(&inbox, &day, &daily::check_items(&todo_dir, today)) {
                        eprintln!("discord-inbox: daily night list for {day} not saved: {e}");
                    }
                }
                for uid in &ids {
                    let sent = match UserId::new(*uid).create_dm_channel(&http).await {
                        Ok(dm) => dm.id.say(&http, &text).await.map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_string()),
                    };
                    if let Err(e) = sent {
                        eprintln!("discord-inbox: daily {slot:?} to {uid} failed: {e}");
                    }
                }
            }
        }));
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
                live: true,
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
            last_notice: Default::default(),
            window: Default::default(),
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

    #[tokio::test]
    async fn install_presence_loop_aborts_previous_loop() {
        let first = tokio::spawn(std::future::pending::<()>());
        let first_abort = first.abort_handle();
        let second = tokio::spawn(std::future::pending::<()>());
        install_presence_loop(first);
        install_presence_loop(second);
        tokio::task::yield_now().await;
        assert!(first_abort.is_finished(), "previous presence loop was not aborted");
    }

    #[tokio::test(start_paused = true)]
    async fn presence_interval_does_not_burst_after_stall() {
        let mut tick = presence_interval();
        tokio::time::advance(Duration::from_secs(3 * 24 * 60 * 60)).await;
        let mut immediate = 0usize;
        while immediate < 1000 {
            if tokio::time::timeout(Duration::from_millis(0), tick.tick())
                .await
                .is_ok()
            {
                immediate += 1;
            } else {
                break;
            }
        }
        assert!(immediate <= 1, "{immediate} presence updates fired back to back after a stall");
    }
}
