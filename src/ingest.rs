//! Decision logic for one DM and for the startup catch-up. main.rs only adapts serenity to `Inbound` and `History`. The live path never moves a cursor; only `catch_up` does, and only over messages that are on disk or in `.retry`.
use crate::allowlist::Allowlist;
use crate::dump::{self, Incoming};
use crate::chat::{Chat, NoChat};
use crate::redact::sensitive;
use crate::vaults::{self, Decision, Vaults};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

pub const PAGE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ignored,
    Skipped,
    Saved,
    Pending,
    Failed,
}

pub const SAVED_MARK: &str = "✅";
pub const PENDING_MARK: &str = "⏳";
pub const FAILED_MARK: &str = "\u{26A0}\u{FE0F}";

/// Leaves exactly `want` on the message among the three marks. Best-effort: errors are logged and change nothing.
async fn mark<M: Inbound>(m: &M, want: &str) {
    let id = m.msg_id();
    let have = m.own_reactions();
    let same = |a: &str, b: &str| a.replace('\u{FE0F}', "") == b.replace('\u{FE0F}', "");
    for stale in [SAVED_MARK, PENDING_MARK, FAILED_MARK] {
        // Always try: a fetched snapshot can predate a mark the live path just added.
        if !same(stale, want) {
            let seen = have.iter().any(|h| same(h, stale));
            if let Err(e) = m.unreact(stale).await {
                if seen {
                    eprintln!("discord-inbox: unreact {stale} on {id} failed: {e}");
                }
            }
        }
    }
    if !have.iter().any(|h| same(h, want)) {
        if let Err(e) = m.react(want).await {
            eprintln!("discord-inbox: react {want} on {id} failed: {e}");
        }
    }
}

/// Clears all three marks, best-effort. Logs only for a mark the message was known to carry.
async fn clear<M: Inbound>(m: &M) {
    let id = m.msg_id();
    let have = m.own_reactions();
    let same = |a: &str, b: &str| a.replace('\u{FE0F}', "") == b.replace('\u{FE0F}', "");
    for stale in [SAVED_MARK, PENDING_MARK, FAILED_MARK] {
        let seen = have.iter().any(|h| same(h, stale));
        if let Err(e) = m.unreact(stale).await {
            if seen {
                eprintln!("discord-inbox: unreact {stale} on {id} failed: {e}");
            }
        }
    }
}

/// The chat model and the vault folders it may file into. `off()` keeps today's behaviour.
pub struct Filer {
    pub chat: Arc<dyn Chat>,
    pub vaults: Vaults,
}

impl Filer {
    pub fn off() -> Filer {
        Filer {
            chat: Arc::new(NoChat),
            vaults: Vaults::new(PathBuf::new()),
        }
    }
}

async fn chat_decision(filer: &Filer, msg: &Incoming) -> Result<Decision, String> {
    let system = vaults::system_prompt();
    let ocr: Vec<String> = msg.ocr.iter().map(|(_, t)| sensitive(t)).collect();
    let user = vaults::user_prompt(&sensitive(&msg.content), &ocr);
    let chat = filer.chat.clone();
    let text = tokio::task::spawn_blocking(move || chat.ask(&system, &user))
        .await
        .map_err(|e| e.to_string())??;
    Ok(vaults::parse_decision(&filer.vaults, &text))
}

/// One reply: where it went, the chat answer, then the receipt verdict if any. Masked and capped.
pub fn combine(filed: Option<&str>, chat: &str, verdict: Option<&str>) -> String {
    let mut out = match filed {
        Some(f) => format!("Filed: {f}"),
        None => "Kept in Inbox".to_string(),
    };
    out.push_str("\n\n");
    out.push_str(chat);
    if let Some(v) = verdict {
        out.push_str("\n\n");
        out.push_str(v);
    }
    let out = sensitive(&out);
    if out.chars().count() > crate::insight::DISCORD_MAX {
        let mut cut: String = out.chars().take(1999).collect();
        cut.push('…');
        return cut;
    }
    out
}

/// One message, as the decision logic sees it.
pub trait Inbound: Send + Sync {
    fn author_id(&self) -> u64;
    fn msg_id(&self) -> u64;
    fn channel_id(&self) -> u64;
    /// UTC, exactly `YYYY-MM-DDTHH:MM:SSZ`.
    fn received(&self) -> String;
    fn content(&self) -> String;
    fn attachment_names(&self) -> Vec<String>;
    fn download(
        &self,
        i: usize,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, String>> + Send;
    /// Adds the bot's reaction `emoji` to this message.
    fn react(&self, emoji: &str) -> impl std::future::Future<Output = Result<(), String>> + Send;
    /// Removes the bot's own reaction `emoji` from this message (nobody else's).
    fn unreact(&self, emoji: &str) -> impl std::future::Future<Output = Result<(), String>> + Send;
    /// The bot's own unicode reactions as this message object carries them: none on a live message, what Discord returned on a fetched one.
    fn own_reactions(&self) -> Vec<String>;
    /// Sends `text` back into the channel this message came from.
    fn reply(&self, text: &str) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

/// The Discord history calls the catch-up needs.
pub trait History: Send + Sync {
    type Msg: Inbound;
    fn dm_channel(
        &self,
        user_id: u64,
    ) -> impl std::future::Future<Output = Result<u64, String>> + Send;
    /// Up to PAGE messages after `after` (the newest PAGE when None), in any order.
    fn page(
        &self,
        channel: u64,
        after: Option<u64>,
    ) -> impl std::future::Future<Output = Result<Vec<Self::Msg>, String>> + Send;
    fn fetch(
        &self,
        channel: u64,
        msg_id: u64,
    ) -> impl std::future::Future<Output = Result<Self::Msg, String>> + Send;
}

/// Gate, download, write and react for one message. Used by the live event and by the catch-up.
pub async fn ingest<M: Inbound>(allow: &Allowlist, inbox: &Path, m: &M) -> Outcome {
    ingest_with(allow, inbox, m, &Filer::off()).await
}

/// One async lock per message id, process-wide. The live event and the catch-up can both reach one DM; each
/// takes its id's lock around the whole ingest, so the second waits and then sees the first's result.
/// One empty entry per id stays for the process lifetime: a few bytes per DM.
static IN_FLIGHT: StdMutex<BTreeMap<u64, Arc<tokio::sync::Mutex<()>>>> = StdMutex::new(BTreeMap::new());

fn id_lock(msg_id: u64) -> Arc<tokio::sync::Mutex<()>> {
    let mut map = IN_FLIGHT.lock().unwrap_or_else(|p| p.into_inner());
    map.entry(msg_id).or_default().clone()
}

pub async fn ingest_with<M: Inbound>(
    allow: &Allowlist,
    inbox: &Path,
    m: &M,
    filer: &Filer,
) -> Outcome {
    // Gate first: a sender not on the allowlist gets no download, write, reaction or log line.
    if !allow.contains(m.author_id()) {
        return Outcome::Ignored;
    }

    let id = m.msg_id();
    // Held to the end of this call. A second caller for the same id waits here, then re-runs the skip check below.
    let lock = id_lock(id);
    let _one = lock.lock().await;
    let received = m.received();
    if (dump::note_saved(&dump::note_path(inbox, &received, id)) || dump::filed_to(inbox, id).is_some())
        && !dump::in_retry(inbox, id)
    {
        return Outcome::Skipped;
    }

    // Keep going after a failed download: the note is written with the attachment as pending.
    let mut attachments: Vec<(String, Option<Vec<u8>>)> = Vec::new();
    for (i, name) in m.attachment_names().into_iter().enumerate() {
        match m.download(i).await {
            Ok(bytes) => attachments.push((name, Some(bytes))),
            Err(e) => {
                eprintln!("discord-inbox: attachment {} of {id} failed: {e}", i + 1);
                attachments.push((name, None));
            }
        }
    }

    // Recorded before the note, so a crash between the two leaves the message owed.
    let pending = attachments.iter().any(|(_, b)| b.is_none());
    if pending {
        if let Err(e) = dump::add_retry(inbox, m.channel_id(), id) {
            eprintln!("discord-inbox: retry record for {id} failed: {e}");
            mark(m, FAILED_MARK).await;
            return Outcome::Failed;
        }
    }

    // Read before the write and outside the dump lock, so the note is still written once. No text is logged.
    let ocr = crate::ocr::read_all(id, &attachments).await;

    let incoming = Incoming {
        msg_id: id,
        author_id: m.author_id(),
        received,
        content: m.content(),
        attachments,
        ocr,
    };
    match dump::accept(allow, inbox, &incoming) {
        Ok(true) => {}
        Ok(false) => return Outcome::Ignored,
        Err(e) => {
            eprintln!("discord-inbox: write {id} failed: {e}");
            // Still recorded in .retry means an earlier note waits on disk: show ⏳, never ⚠️.
            return if pending || dump::in_retry(inbox, id) {
                mark(m, PENDING_MARK).await;
                Outcome::Pending
            } else {
                mark(m, FAILED_MARK).await;
                Outcome::Failed
            };
        }
    }

    if pending {
        mark(m, PENDING_MARK).await;
        return Outcome::Pending;
    }
    if let Err(e) = dump::remove_retry(inbox, id) {
        eprintln!("discord-inbox: retry clear for {id} failed: {e}");
    }
    match chat_decision(filer, &incoming).await {
        Err(e) => {
            eprintln!("discord-inbox: chat for {id} failed: {e}");
            mark(m, SAVED_MARK).await;
            if let Some(text) = crate::insight::compose(&incoming).await {
                if let Err(e) = m.reply(&text).await {
                    eprintln!("discord-inbox: reply on {id} failed: {e}");
                }
            }
        }
        Ok(d) => {
            let filed: Option<String> = d.target.and_then(|t| {
                let dir = filer.vaults.dest_dir(t)?;
                let name = vaults::note_name(&incoming.received, &d.title);
                match dump::file_away(
                    inbox,
                    &incoming,
                    &dir,
                    &name,
                    &vaults::front_extra(t, &d.tags),
                    &vaults::hub_line(t),
                ) {
                    Ok(_) => Some(format!("{}/{}", t.folder, name)),
                    Err(e) => {
                        eprintln!("discord-inbox: file {id} failed: {e}");
                        None
                    }
                }
            });
            let verdict = crate::insight::compose(&incoming).await;
            let text = combine(filed.as_deref(), &d.reply, verdict.as_deref());
            match m.reply(&text).await {
                Ok(()) => clear(m).await,
                Err(e) => {
                    eprintln!("discord-inbox: reply on {id} failed: {e}");
                    mark(m, SAVED_MARK).await;
                }
            }
        }
    }
    Outcome::Saved
}

/// Startup catch-up: first the retry list, then each allowlisted DM channel from its own cursor.
pub async fn catch_up<H: History>(allow: &Allowlist, inbox: &Path, h: &H) {
    catch_up_with(allow, inbox, h, &Filer::off()).await
}

pub async fn catch_up_with<H: History>(allow: &Allowlist, inbox: &Path, h: &H, filer: &Filer) {
    for (channel, id) in dump::retry_entries(inbox) {
        match h.fetch(channel, id).await {
            Ok(m) => {
                let _ = ingest_with(allow, inbox, &m, filer).await;
            }
            Err(e) => eprintln!("discord-inbox: retry {id} not fetchable: {e}"),
        }
    }

    for user in allow.ids() {
        let channel = match h.dm_channel(user).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("discord-inbox: dm channel failed: {e}");
                continue;
            }
        };
        let start = dump::read_cursor(inbox, channel);
        let mut after = start;
        // The cursor only moves over a contiguous run that is on disk or in .retry.
        let mut safe = start;
        let mut blocked = false;
        loop {
            let mut page = match h.page(channel, after).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("discord-inbox: history failed: {e}");
                    break;
                }
            };
            page.sort_by_key(|m| m.msg_id());
            for m in &page {
                match ingest_with(allow, inbox, m, filer).await {
                    Outcome::Failed => blocked = true,
                    Outcome::Ignored => {}
                    Outcome::Skipped | Outcome::Saved | Outcome::Pending => {
                        if !blocked {
                            safe = Some(m.msg_id());
                        }
                    }
                }
            }
            let Some(max) = page.last().map(|m| m.msg_id()) else {
                break;
            };
            if page.len() < PAGE || after.is_some_and(|a| max <= a) {
                break;
            }
            after = Some(max);
        }
        if safe != start {
            if let Some(id) = safe {
                if let Err(e) = dump::write_cursor(inbox, channel, id) {
                    eprintln!("discord-inbox: cursor for channel {channel} failed: {e}");
                }
            }
        }
    }
}
