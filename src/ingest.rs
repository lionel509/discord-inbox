//! Decision logic for one DM and for the startup catch-up. main.rs only adapts serenity to `Inbound` and `History`. The live path never moves a cursor; only `catch_up` does, and only over messages that are on disk or in `.retry`.
use crate::allowlist::Allowlist;
use crate::dump::{self, Incoming};
use crate::chat::{Chat, NoChat};
use crate::redact::sensitive;
use crate::vaults::{self, Decision, Vaults};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

pub const PAGE: usize = 100;
pub const NOTICE_GAP: std::time::Duration = std::time::Duration::from_secs(600);

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
/// The bot's reaction on a message whose text was filed: the only sign of filing (#67). Never cleared by `clear`.
pub const FILED_MARK: &str = "\u{1F43E}";
/// Turns kept per channel for the warm-reply prompt (#67). In memory only.
pub const WINDOW_TURNS: usize = 6;
const WINDOW_AGE_SECS: i64 = 30 * 60;
pub const WINDOW_CHARS: usize = 400;

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
    /// When the last chat-failure notice went out; cleared when a chat call succeeds (#40).
    pub last_notice: std::sync::Mutex<Option<std::time::Instant>>,
    /// The last few DM turns per channel, prefixed to the prompts (#67).
    pub window: Window,
}

struct Turn {
    /// The triggering message's Discord id: snowflakes order sub-second, the rfc3339 stamp does not (#94).
    id: u64,
    at: chrono::DateTime<chrono::Utc>,
    him: String,
    her: String,
}

/// One channel's turns, plus the stamp of the last `/clear` there (#92).
#[derive(Default)]
struct Lane {
    turns: VecDeque<Turn>,
    cleared: Option<u64>,
}

/// A message's UTC rfc3339 stamp; one that does not parse counts as now.
fn stamp(received: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(received)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

/// The recent DM turns per channel (#67). Masked and capped on the way in; read back oldest first.
#[derive(Default)]
pub struct Window(StdMutex<BTreeMap<u64, Lane>>);

impl Window {
    /// Records one turn of message `msg_id`. `received` is its UTC rfc3339 stamp (used for the age window); a stamp that does not parse counts as now.
    pub fn push(&self, channel: u64, msg_id: u64, received: &str, him: &str, her: &str) {
        let at = stamp(received);
        let cut = |s: &str| sensitive(s).chars().take(WINDOW_CHARS).collect::<String>();
        let turn = Turn { id: msg_id, at, him: cut(him), her: cut(her) };
        let mut map = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let lane = map.entry(channel).or_default();
        // A turn of a message at or before a `/clear` is refused: a reply still in flight when the clear landed (#92).
        // Compared by message id, so a message sent after the clear is kept even in the same second (#94).
        if lane.cleared.is_some_and(|c| msg_id <= c) {
            return;
        }
        let q = &mut lane.turns;
        // Keep the queue sorted by stamp: a retried message can arrive after newer turns.
        let pos = q.iter().position(|t| t.at > turn.at).unwrap_or(q.len());
        q.insert(pos, turn);
        while q.len() > WINDOW_TURNS {
            q.pop_front();
        }
    }

    /// Drops the turns of `channel` from messages at or before the `/clear` `msg_id`, and refuses later pushes from them (#92, #94).
    pub fn forget(&self, channel: u64, msg_id: u64) {
        let mut map = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let lane = map.entry(channel).or_default();
        lane.turns.retain(|t| t.id > msg_id);
        lane.cleared = Some(lane.cleared.map_or(msg_id, |c| c.max(msg_id)));
    }

    /// The turns of `channel` within the window's age of now, oldest first, as `(him, her)`.
    pub fn recent(&self, channel: u64) -> Vec<(String, String)> {
        self.recent_at(channel, chrono::Utc::now())
    }

    /// The turns of `channel` within the window's age of `now` (the incoming message's own time), oldest first.
    pub fn recent_at(&self, channel: u64, now: chrono::DateTime<chrono::Utc>) -> Vec<(String, String)> {
        let cutoff = now - chrono::Duration::seconds(WINDOW_AGE_SECS);
        let map = self.0.lock().unwrap_or_else(|p| p.into_inner());
        map.get(&channel)
            .map(|l| l.turns.iter().filter(|t| t.at >= cutoff && t.at <= now).map(|t| (t.him.clone(), t.her.clone())).collect())
            .unwrap_or_default()
    }
}

impl Filer {
    pub fn off() -> Filer {
        Filer {
            chat: Arc::new(NoChat),
            vaults: Vaults::new(PathBuf::new()),
            last_notice: Default::default(),
            window: Default::default(),
        }
    }
}

/// The sender's local date for a UTC `received` stamp (#50). Falls back to the UTC date when it does not parse.
fn local_date(received: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(received) {
        Ok(dt) => crate::when::local_stamp(dt.with_timezone(&chrono::Utc))[..10].to_string(),
        Err(_) => received.get(..10).unwrap_or(received).to_string(),
    }
}

async fn chat_decision(filer: &Filer, msg: &Incoming, turns: &[(String, String)]) -> Result<Decision, String> {
    let system = vaults::system_prompt();
    let user = prompt_text(msg, turns, vaults::PROMPT_CHARS);
    let chat = filer.chat.clone();
    let text = tokio::task::spawn_blocking(move || chat.ask(&system, &user))
        .await
        .map_err(|e| e.to_string())??;
    Ok(vaults::parse_decision(&filer.vaults, &text))
}

/// Appends a list message to its running note. `None` (no running note, attachments, or a failed append) means the caller files it as today.
fn list_append(filer: &Filer, inbox: &Path, incoming: &Incoming, d: &Decision) -> Option<()> {
    let r = d.running?;
    if !incoming.attachments.is_empty() {
        return None;
    }
    let note = filer.vaults.running_path(r)?;
    match dump::append_running(inbox, incoming, &note, &incoming.received[..10]) {
        Ok(()) => Some(()),
        Err(e) => {
            eprintln!("discord-inbox: append {} failed: {e}", incoming.msg_id);
            None
        }
    }
}

/// One reply: the model's text, then one plain line saying where it went (when there is one), then the receipt verdict. Masked and capped (#45).
pub fn combine(line: Option<&str>, chat: &str, verdict: Option<&str>) -> String {
    match line {
        Some(l) => reply_text(&format!("{chat}\n\n{l}"), verdict),
        None => reply_text(chat, verdict),
    }
}

/// The reply text: `chat`, then the verdict if any, masked and capped. No status label goes first (#45).
fn reply_text(chat: &str, verdict: Option<&str>) -> String {
    let mut out = chat.to_string();
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

/// How often "typing" is re-sent while a model call runs. Discord's indicator lasts about 10 s (#45).
pub const TYPING_EVERY: std::time::Duration = std::time::Duration::from_secs(8);
/// The longest a typing call may hold up the reply. Typing is best-effort (#45).
pub const TYPING_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Runs `work`, re-sending the typing indicator every `TYPING_EVERY` until it finishes. Live DMs only: catch-up never types (#45).
pub(crate) async fn typing_while<M: Inbound, F: std::future::Future>(m: &M, work: F) -> F::Output {
    if !m.live() {
        return work.await;
    }
    // Sent before the request starts, then again every TYPING_EVERY until the work is done.
    let _ = tokio::time::timeout(TYPING_WAIT, m.typing()).await;
    tokio::pin!(work);
    let start = tokio::time::Instant::now() + TYPING_EVERY;
    let mut every = tokio::time::interval_at(start, TYPING_EVERY);
    loop {
        tokio::select! {
            out = &mut work => return out,
            _ = every.tick() => {
                let _ = tokio::time::timeout(TYPING_WAIT, m.typing()).await;
            }
        }
    }
}

/// The reply to a DM with nothing to read (#46).
pub const BLANK_REPLY: &str = "Got it.";
/// How much of a text attachment is read as the message (#46).
pub const ATTACH_TEXT_CHARS: usize = 20_000;

/// The text of a `.txt` or `.md` attachment, from the bytes that were saved, up to ATTACH_TEXT_CHARS on a char boundary. `None` for other files or binary (a NUL byte); invalid UTF-8 is decoded lossily (#46).
fn attachment_text(name: &str, bytes: &[u8]) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if !(lower.ends_with(".txt") || lower.ends_with(".md")) {
        return None;
    }
    if bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    Some(text.chars().take(ATTACH_TEXT_CHARS).collect())
}

/// The typed text, then each readable text attachment after it. Blank when neither has text (#46).
fn message_text(typed: &str, attachments: &[(String, Option<Vec<u8>>)]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !typed.trim().is_empty() {
        parts.push(typed.to_string());
    }
    for (name, bytes) in attachments {
        if let Some(t) = bytes.as_deref().and_then(|b| attachment_text(name, b)) {
            if !t.trim().is_empty() {
                parts.push(t);
            }
        }
    }
    parts.join("\n\n")
}

/// What the model reads for a DM: the redacted text, then each redacted OCR block, behind the window (#67, #73). Filing caps it at
/// `PROMPT_CHARS`, a question at `LOOKUP_CHARS` so a long pasted question keeps its tail (#75).
fn prompt_text(msg: &Incoming, turns: &[(String, String)], limit: usize) -> String {
    let ocr: Vec<String> = msg.ocr.iter().map(|(_, t)| sensitive(t)).collect();
    vaults::with_window(turns, &vaults::user_prompt_within(&sensitive(&msg.content), &ocr, limit))
}

/// The text a DM carries for the window: what he typed, then each attachment by name only (never its OCR text) (#67).
fn turn_text(incoming: &Incoming) -> String {
    let mut out = incoming.content.clone();
    for (name, _) in &incoming.attachments {
        out.push_str(&format!("\n[attachment: {name}]"));
    }
    out
}

/// A forward has no text of its own: the snapshot text stands in when the typed text is blank (#67).
pub fn forwarded_text(content: &str, snapshots: &[String]) -> String {
    if !content.trim().is_empty() {
        return content.to_string();
    }
    snapshots.iter().filter(|s| !s.trim().is_empty()).cloned().collect::<Vec<_>>().join("\n\n")
}

/// One message, as the decision logic sees it.
pub trait Inbound: Send + Sync {
    fn author_id(&self) -> u64;
    fn msg_id(&self) -> u64;
    fn channel_id(&self) -> u64;
    /// Local time with offset, `YYYY-MM-DDTHH:MM:SS±HH:MM`; dump and vaults slice the first 19 chars.
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
    /// Sends a rich reply (embed, optional image) into the same channel. Default: its plain-text fallback.
    fn reply_rich(&self, r: &crate::rich::Rich) -> impl std::future::Future<Output = Result<(), String>> + Send {
        let t = r.text.clone();
        async move { self.reply(&t).await }
    }
    /// False for a message replayed from history (catch-up). Only live messages show typing (#45).
    fn live(&self) -> bool {
        true
    }
    /// Shows "typing" in the message's channel, best-effort. Default: nothing.
    fn typing(&self) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
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
    if (dump::note_saved(&dump::note_path(inbox, &received, id)) || dump::saved_id(inbox, id) || dump::filed_to(inbox, id).is_some())
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
        content: message_text(&m.content(), &attachments),
        attachments,
        ocr,
    };
    // Daily check-in (#135): while the night marker reads `sent`, a text-only DM answers the check-in. Before the Inbox write, so a check-in reply files no note; ahead of the calendar hook so "done with the lab" strikes the to-do, not a calendar event.
    // A catch-up replay of a message older than the night DM is not the reply: it files as usual.
    // A message with no letters or digits (a zero-width space, a lone emoji) is not a reply to read.
    if incoming.attachments.is_empty() && incoming.content.chars().any(|c| c.is_alphanumeric()) {
        let open = crate::daily::awaiting(inbox, chrono::Local::now().naive_local());
        if let Some(day) = open.filter(|d| crate::daily::after_send(inbox, d, &incoming.received)) {
            if let Ok(day_date) = chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d") {
                let sent = crate::daily::load_list(inbox, &day);
                let deps = crate::daily::Deps {
                    todo_dir: filer.vaults.root().join(crate::todo::DIR),
                    cal: crate::calendar::Cal::from_env(),
                    chat: filer.chat.clone(),
                };
                let message = incoming.content.clone();
                let r = typing_while(m, tokio::task::spawn_blocking(move || crate::daily::answer(&deps, day_date, &message, sent)))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r);
                match r {
                    Ok(Some(text)) => {
                        if let Err(e) = crate::daily::answered(inbox, &day) {
                            eprintln!("discord-inbox: daily check-in marker for {id} not updated: {e}");
                        }
                        match m.reply(&reply_text(&text, None)).await {
                            Ok(()) => clear(m).await,
                            Err(e) => {
                                eprintln!("discord-inbox: reply on {id} failed: {e}");
                                mark(m, SAVED_MARK).await;
                            }
                        }
                        return Outcome::Saved;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        // Said out loud, then the message is filed as usual below.
                        // No reply here: the filer below answers the message, so a model outage gets one reply.
                        eprintln!("discord-inbox: daily check-in for {id} failed: {e}");
                    }
                }
            }
        }
    }
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
    // Nothing to read (no text, no OCR text): the note is saved and acknowledged, the model is not called (#46).
    if incoming.content.trim().is_empty() && incoming.ocr.is_empty() {
        match m.reply(BLANK_REPLY).await {
            Ok(()) => clear(m).await,
            Err(e) => {
                eprintln!("discord-inbox: reply on {id} failed: {e}");
                mark(m, SAVED_MARK).await;
            }
        }
        return Outcome::Saved;
    }
    // Typed commands (#92): `/commands` lists them, `/clear` blanks the chat and forgets our last few turns.
    if incoming.attachments.is_empty() {
        if let Some(cmd) = crate::commands::parse(&incoming.content) {
            let text = match cmd {
                crate::commands::Slash::Commands => crate::commands::COMMANDS_TEXT.to_string(),
                crate::commands::Slash::Clear => {
                    filer.window.forget(m.channel_id(), id);
                    crate::commands::clear_text()
                }
            };
            match m.reply(&text).await {
                Ok(()) => clear(m).await,
                Err(e) => {
                    eprintln!("discord-inbox: reply on {id} failed: {e}");
                    mark(m, SAVED_MARK).await;
                }
            }
            return Outcome::Saved;
        }
    }
    // Pantry commands (#50) are answered from the vault notes, before any model call.
    if incoming.attachments.is_empty() {
        if let Some(cmd) = crate::pantry::command(&incoming.content) {
            let today = local_date(&incoming.received);
            let root = filer.vaults.root().to_path_buf();
            let text = tokio::task::spawn_blocking(move || crate::pantry::run(&root, cmd, &today))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
                .unwrap_or_else(|e| {
                    eprintln!("discord-inbox: pantry for {id} failed: {e}");
                    format!("I couldn't update the pantry: {e}")
                });
            match m.reply(&text).await {
                Ok(()) => clear(m).await,
                Err(e) => {
                    eprintln!("discord-inbox: reply on {id} failed: {e}");
                    mark(m, SAVED_MARK).await;
                }
            }
            return Outcome::Saved;
        }
    }
    // School-calendar check-off (#95): "done with <task>" marks the matching event. Answered before any chat call; no match falls through to chat.
    if incoming.attachments.is_empty() {
        if crate::calendar::wanted(&incoming.content).is_some() {
            if let Some(cal) = crate::calendar::Cal::from_env() {
                let today = local_date(&incoming.received);
                let chat = filer.chat.clone();
                let message = incoming.content.clone();
                let r = typing_while(m, tokio::task::spawn_blocking(move || crate::calendar::finish(chat.as_ref(), &cal, &message, &today)))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r);
                let text = match r {
                    Ok(crate::calendar::Finish::NoMatch) => {
                        eprintln!("discord-inbox: calendar for {id}: no match, chatting");
                        None
                    }
                    Ok(crate::calendar::Finish::Done(t) | crate::calendar::Finish::Ask(t)) => Some(t),
                    Err(e) => {
                        eprintln!("discord-inbox: calendar for {id} failed: {e}");
                        Some("I couldn't reach your calendar just now 😿 Your message is saved in your Inbox.".to_string())
                    }
                };
                if let Some(text) = text {
                    match m.reply(&reply_text(&text, None)).await {
                        Ok(()) => clear(m).await,
                        Err(e) => {
                            eprintln!("discord-inbox: reply on {id} failed: {e}");
                            mark(m, SAVED_MARK).await;
                        }
                    }
                    return Outcome::Saved;
                }
            }
        }
    }
    // Aged against the incoming message's own stamp, so a replayed DM sees the turns around it.
    let now = chrono::DateTime::parse_from_rfc3339(&incoming.received)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let turns = filer.window.recent_at(m.channel_id(), now);
    match typing_while(m, chat_decision(filer, &incoming, &turns)).await {
        Err(e) => {
            eprintln!("discord-inbox: chat for {id} failed: {e}");
            let verdict = crate::insight::compose(&incoming).await;
            let first = {
                let mut last = filer.last_notice.lock().unwrap_or_else(|p| p.into_inner());
                let first = last.is_none_or(|t| t.elapsed() >= NOTICE_GAP);
                if first {
                    *last = Some(std::time::Instant::now());
                }
                first
            };
            mark(m, SAVED_MARK).await;
            let text = if first {
                Some(reply_text("Sorry, my brain's a little foggy right now. Your message is saved in your Inbox. For the next 10 minutes I'll just ✅ new ones and catch up after.", verdict.as_deref()))
            } else {
                verdict.map(|v| reply_text("Still a little foggy. This one's saved in your Inbox too.", Some(&v)))
            };
            if let Some(text) = text {
                if let Err(e) = m.reply(&text).await {
                    eprintln!("discord-inbox: reply on {id} failed: {e}");
                }
            }
        }
        Ok(d) => {
            *filer.last_notice.lock().unwrap_or_else(|p| p.into_inner()) = None;
            let verdict = crate::insight::compose(&incoming).await;
            // A reply that promises a lookup, on a message that filed nothing, is a question too: the bot cannot come back later (#73).
            let promised = d.target.is_none() && vaults::promised(&d.reply);
            if d.answer || vaults::is_question(&incoming.content) || promised {
                // A question: the agent searches and reads the notes itself (#49), file nothing, keep the Inbox note.
                let lookup = crate::tools::Lookup::new(filer.vaults.root().to_path_buf(), inbox.to_path_buf());
                let question = prompt_text(&incoming, &turns, vaults::LOOKUP_CHARS);
                let chat = filer.chat.clone();
                let looked = typing_while(m, tokio::task::spawn_blocking(move || crate::agent::answer(chat.as_ref(), &lookup, &question)))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r)
                    .and_then(|a| if a.text.trim().is_empty() { Err("empty answer".to_string()) } else { Ok(a) });
                let body = match looked {
                    Ok(a) => {
                        eprintln!(
                            "discord-inbox: answer for {id}: {} lookups, {}+{} tokens{}",
                            a.calls,
                            a.usage.prompt,
                            a.usage.completion,
                            a.limited.map(|l| format!(", limit: {l}")).unwrap_or_default()
                        );
                        a.text
                    }
                    Err(e) => {
                        eprintln!("discord-inbox: lookup for {id} failed: {e}");
                        let kept = vaults::without_promises(&d.reply);
                        let filed_in = d.target.map(|t| format!("Filed it in {}.", t.folder));
                        let said = if kept.is_empty() && filed_in.is_some() { filed_in.as_deref().unwrap() } else if kept.is_empty() { "I tried to look that up for you and couldn't get to your notes." } else { kept.as_str() };
                        format!("{said}\n\n(I couldn't peek at your notes just now, sorry!)")
                    }
                };
                let text = reply_text(&body, verdict.as_deref());
                match m.reply(&text).await {
                    Ok(()) => {
                        clear(m).await;
                        filer.window.push(m.channel_id(), id, &incoming.received, &turn_text(&incoming), &text);
                    }
                    Err(e) => {
                        eprintln!("discord-inbox: reply on {id} failed: {e}");
                        mark(m, SAVED_MARK).await;
                    }
                }
                return Outcome::Saved;
            }
            // A list message with no attachment is appended to its running note; otherwise it is filed as today.
            let filed = list_append(filer, inbox, &incoming, &d).is_some()
                || d.target.is_some_and(|t| match filer.vaults.dest_dir(t) {
                    None => false,
                    Some(dir) => match dump::file_away(inbox, &incoming, &dir, &vaults::note_name(&incoming.received, &d.title), &vaults::front_extra(t, &d.tags), &vaults::hub_line(t)) {
                        Ok(_) => true,
                        Err(e) => {
                            eprintln!("discord-inbox: file {id} failed: {e}");
                            false
                        }
                    },
                });
            // A target the model picked that did not take the note: say so, in the same one-line shape.
            let line = (d.target.is_some() && !filed).then_some("Hmm, I couldn't file that one, so it's safe in your Inbox for now.");
            let text = combine(line, &d.reply, verdict.as_deref());
            match m.reply(&text).await {
                Ok(()) => {
                    clear(m).await;
                    if filed {
                        if let Err(e) = m.react(FILED_MARK).await {
                            eprintln!("discord-inbox: react on {id} failed: {e}");
                        }
                    }
                    filer.window.push(m.channel_id(), id, &incoming.received, &turn_text(&incoming), &text);
                }
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

/// Stormy's catch-up: `catch_up_each` with `ingest_with` as the per-message handler.
pub async fn catch_up_with<H: History>(allow: &Allowlist, inbox: &Path, h: &H, filer: &Filer) {
    catch_up_each(allow, inbox, h, &Stormy { allow, inbox, filer }).await
}

/// Stormy's per-message handler: the full `ingest_with` path.
struct Stormy<'a> {
    allow: &'a Allowlist,
    inbox: &'a Path,
    filer: &'a Filer,
}

impl<M: Inbound> Handle<M> for Stormy<'_> {
    fn handle(&self, m: &M) -> impl std::future::Future<Output = Outcome> + Send {
        ingest_with(self.allow, self.inbox, m, self.filer)
    }
}

/// Per-message handler the generic catch-up drives (U4). Stormy's is `ingest_with`; Pip's is `pip::PipHandler`.
pub trait Handle<M: Inbound>: Sync {
    fn handle(&self, m: &M) -> impl std::future::Future<Output = Outcome> + Send;
}

/// Generic catch-up with a per-message handler (U4): the retry list, then each allowlisted DM channel from its cursor.
pub async fn catch_up_each<H: History, D: Handle<H::Msg>>(allow: &Allowlist, state: &Path, h: &H, d: &D) {
    for (channel, id) in dump::retry_entries(state) {
        match h.fetch(channel, id).await {
            Ok(m) => {
                let _ = d.handle(&m).await;
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
        let start = dump::read_cursor(state, channel);
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
                match d.handle(m).await {
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
                if let Err(e) = dump::write_cursor(state, channel, id) {
                    eprintln!("discord-inbox: cursor for channel {channel} failed: {e}");
                }
            }
        }
    }
}
