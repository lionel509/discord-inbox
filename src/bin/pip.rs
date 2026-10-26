//! Pip, the nutrition DM bot (U4): the same serenity wiring as discord-inbox, with Pip's own token and handler.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::checkin;
use discord_inbox::ingest::catch_up_each;
use discord_inbox::live::{Live, LiveMsg};
use discord_inbox::pip::{self, Config, PipHandler, PRESENCE};
use serenity::all::{ActivityData, Client, Context, EventHandler, GatewayIntents, Message, OnlineStatus, Ready, UserId};
use serenity::async_trait;
use std::fs;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::{interval, MissedTickBehavior};

const KEYCHAIN_SERVICE: &str = "pip";
const KEYCHAIN_ACCOUNT: &str = "pip";

/// Check-in tick: 60 s, so the 21:00 rule holds within a minute of waking.
const CHECKIN_TICK: Duration = Duration::from_secs(60);
/// The one live check-in loop. Each `ready` replaces it and aborts the old one.
static CHECKIN_LOOP: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

fn install_checkin_loop(handle: JoinHandle<()>) {
    let previous = CHECKIN_LOOP.lock().unwrap().replace(handle);
    if let Some(old) = previous {
        old.abort();
    }
}

struct Handler {
    allow: Arc<Allowlist>,
    cfg: Arc<Config>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _: Ready) {
        eprintln!("pip: connected");
        ctx.set_presence(Some(ActivityData::custom(PRESENCE)), OnlineStatus::Idle);

        // Stop the old check-in loop first, so no tick from a previous `ready` runs during catch-up.
        if let Some(old) = CHECKIN_LOOP.lock().unwrap().take() {
            old.abort();
        }

        // Catch-up before the check-in loop starts: a 21:00 tick must not beat the DMs that were sent while asleep.
        catch_up_each(
            &self.allow,
            &self.cfg.nutrition_dir,
            &Live { http: ctx.http.clone() },
            &PipHandler { allow: &self.allow, cfg: &self.cfg },
        )
        .await;

        // Every `ready` (including a re-identify) replaces the check-in loop, so it never holds a dead shard's Context.
        let http = ctx.http.clone();
        let allow = self.allow.clone();
        let dir = self.cfg.nutrition_dir.clone();
        install_checkin_loop(tokio::spawn(async move {
            let mut tick = interval(CHECKIN_TICK);
            tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            // The tick after a sleep is skipped, so RESUME replays and catch-up land before any check-in is judged.
            let mut prev = chrono::Local::now().naive_local();
            loop {
                tick.tick().await;
                let now = chrono::Local::now().naive_local();
                let woke = checkin::just_woke(prev, now);
                prev = now;
                if woke {
                    continue;
                }
                if let Some(day) = checkin::due(&dir, now) {
                    let result = checkin::send(&dir, &day, |text: String| {
                        let http = http.clone();
                        let allow = allow.clone();
                        async move {
                            for user in allow.ids() {
                                let dm = UserId::new(user).create_dm_channel(&http).await.map_err(|e| e.to_string())?;
                                dm.id.say(&http, &text).await.map_err(|e| e.to_string())?;
                            }
                            Ok(())
                        }
                    })
                    .await;
                    match result {
                        Ok(()) => eprintln!("pip: check-in sent for {day}"),
                        Err(e) => eprintln!("pip: check-in for {day} failed: {e}"),
                    }
                }
            }
        }));
    }

    async fn message(&self, ctx: Context, msg: Message) {
        let _ = pip::handle(&self.allow, &self.cfg, &LiveMsg { http: ctx.http.clone(), msg, live: true }).await;
    }
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("pip: {msg}");
    ExitCode::FAILURE
}

#[tokio::main]
async fn main() -> ExitCode {
    // 1. Allowlist first: the same file as discord-inbox, so the same user IDs are allowed.
    let allow = match Allowlist::load(&allowlist::default_path()) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    // 2. Pip's own settings: the nutrition folder must be absolute and is required.
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    let dir = &cfg.nutrition_dir;
    if let Err(e) = fs::create_dir_all(dir) {
        return fail(format!("nutrition dir {} not creatable: {e}", dir.display()));
    }

    // Single instance: a second Pip would race this one on the same notes. Held until main returns.
    let lock_path = dir.join(".lock");
    let _lock = match fs::OpenOptions::new().create(true).write(true).open(&lock_path) {
        Ok(f) => match f.try_lock() {
            Ok(()) => f,
            Err(fs::TryLockError::WouldBlock) => {
                return fail(format!("already running: {} is held by another instance", lock_path.display()))
            }
            Err(fs::TryLockError::Error(e)) => return fail(format!("lock {} failed: {e}", lock_path.display())),
        },
        Err(e) => return fail(format!("lock {} not openable: {e}", lock_path.display())),
    };

    // 3. Token from the login Keychain (item pip). Never printed.
    let token = match security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s.trim().to_string(),
            Err(_) => return fail("keychain item pip is not UTF-8"),
        },
        Err(e) => return fail(format!("keychain item pip not readable: {e}")),
    };

    // 4. DMs only: no guild intents, no message content intent.
    let mut client = match Client::builder(&token, GatewayIntents::DIRECT_MESSAGES)
        .event_handler(Handler { allow: Arc::new(allow), cfg: Arc::new(cfg) })
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
