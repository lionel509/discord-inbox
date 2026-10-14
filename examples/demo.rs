//! Offline demo of the allowlist gate, the Inbox writer and the retry list. No Discord, no token.
//! Run with `sh examples/demo.sh` (also runs the refuse-to-start check).
//!
//! The allowlist is written to a temp file and loaded through `allowlist::default_path()`
//! (via DISCORD_INBOX_ALLOWED_IDS_FILE), the same path the bot reads at startup.
//! Messages go through the real `ingest::ingest` and `ingest::catch_up`, with an in-file fake
//! of `Inbound` and `History`. NOT exercised: serenity's adapters in main.rs (`LiveMsg`, `Live`),
//! which need a live gateway.
//!
//! Real IDs come from the environment (DEMO_MAIN_ID, DEMO_ALT_ID) and are never printed.
//! Without them the demo uses stand-in IDs and says so.
use discord_inbox::allowlist::{self, Allowlist};
use discord_inbox::dump::{self, read_cursor, retry_entries, sha256_hex};
use discord_inbox::ingest::{
    catch_up, ingest, History, Inbound, Outcome, FAILED_MARK, PAGE, PENDING_MARK, SAVED_MARK,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const STAND_IN_MAIN: u64 = 100000000000000001;
const STAND_IN_ALT: u64 = 100000000000000002;
const THIRD_ID: u64 = 999999999999999999;
const MAIN_CH: u64 = 7001;
const ALT_CH: u64 = 7002;
const THIRD_CH: u64 = 7009;

fn show(path: &Path, tmp: &str) -> String {
    path.display().to_string().replace(tmp, "<tmp>")
}

/// The mark a message carries, in Lionel's terms. Empty list means no mark.
fn mark_text(marks: &[String]) -> String {
    let has = |m: &str| marks.iter().any(|x| x == m);
    if has(SAVED_MARK) {
        format!("{SAVED_MARK} (logged in the vault)")
    } else if has(PENDING_MARK) {
        format!("{PENDING_MARK} (cached, waiting to be logged)")
    } else if has(FAILED_MARK) {
        format!("{FAILED_MARK} (could not be written)")
    } else {
        "(no mark)".to_string()
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn id_from_env(var: &str, fallback: u64) -> (u64, bool) {
    match std::env::var(var) {
        Ok(v) => (v.trim().parse().expect("DEMO_*_ID must be a number"), true),
        Err(_) => (fallback, false),
    }
}

/// Shared by every fake message: the CDN switch and per-message `(downloads, reacts)`.
#[derive(Default)]
struct World {
    cdn_down: AtomicBool,
    calls: Mutex<HashMap<u64, (usize, usize)>>,
    marks: Mutex<HashMap<u64, Vec<String>>>,
}

impl World {
    fn count(&self, id: u64, react: bool) {
        let mut calls = self.calls.lock().unwrap();
        let entry = calls.entry(id).or_insert((0, 0));
        if react {
            entry.1 += 1;
        } else {
            entry.0 += 1;
        }
    }
    fn downloads(&self, id: u64) -> usize {
        self.calls.lock().unwrap().get(&id).map_or(0, |c| c.0)
    }
    fn marks(&self, id: u64) -> Vec<String> {
        self.marks.lock().unwrap().get(&id).cloned().unwrap_or_default()
    }
    fn reacts(&self, id: u64) -> usize {
        self.calls.lock().unwrap().get(&id).map_or(0, |c| c.1)
    }
}

#[derive(Clone)]
struct DemoMsg {
    author: u64,
    channel: u64,
    id: u64,
    received: &'static str,
    content: &'static str,
    files: Vec<(String, Vec<u8>)>,
    world: Arc<World>,
}

impl Inbound for DemoMsg {
    fn author_id(&self) -> u64 {
        self.author
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        self.channel
    }
    fn received(&self) -> String {
        self.received.to_string()
    }
    fn content(&self) -> String {
        self.content.to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.files.iter().map(|f| f.0.clone()).collect()
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.world.count(self.id, false);
        let (name, bytes) = &self.files[i];
        if self.world.cdn_down.load(Ordering::SeqCst) && name.starts_with("bad") {
            return Err("cdn 503".to_string());
        }
        Ok(bytes.clone())
    }
    async fn react(&self, emoji: &str) -> Result<(), String> {
        self.world.count(self.id, true);
        let mut marks = self.world.marks.lock().unwrap();
        let v = marks.entry(self.id).or_default();
        if !v.iter().any(|e| e == emoji) {
            v.push(emoji.to_string());
        }
        Ok(())
    }
    async fn unreact(&self, emoji: &str) -> Result<(), String> {
        if let Some(v) = self.world.marks.lock().unwrap().get_mut(&self.id) {
            v.retain(|e| e != emoji);
        }
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        self.world.marks(self.id)
    }
    async fn reply(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

struct DemoHistory {
    main_id: u64,
    alt_id: u64,
    msgs: Vec<DemoMsg>,
}

impl History for DemoHistory {
    type Msg = DemoMsg;

    async fn dm_channel(&self, user_id: u64) -> Result<u64, String> {
        if user_id == self.main_id {
            Ok(MAIN_CH)
        } else if user_id == self.alt_id {
            Ok(ALT_CH)
        } else {
            Err("no dm channel".to_string())
        }
    }

    async fn page(&self, channel: u64, after: Option<u64>) -> Result<Vec<DemoMsg>, String> {
        let mut picked: Vec<DemoMsg> = self
            .msgs
            .iter()
            .filter(|m| m.channel == channel && after.is_none_or(|a| m.id > a))
            .cloned()
            .collect();
        picked.sort_by_key(|m| m.id);
        match after {
            None => {
                let start = picked.len().saturating_sub(PAGE);
                picked = picked.split_off(start);
            }
            Some(_) => picked.truncate(PAGE),
        }
        picked.reverse();
        Ok(picked)
    }

    async fn fetch(&self, channel: u64, msg_id: u64) -> Result<DemoMsg, String> {
        self.msgs
            .iter()
            .find(|m| m.id == msg_id && m.channel == channel)
            .cloned()
            .ok_or_else(|| "unknown message".to_string())
    }
}

fn demo_msg(
    world: &Arc<World>,
    author: u64,
    channel: u64,
    id: u64,
    received: &'static str,
    content: &'static str,
    files: &[(&str, &[u8])],
) -> DemoMsg {
    DemoMsg {
        author,
        channel,
        id,
        received,
        content,
        files: files
            .iter()
            .map(|(n, b)| (n.to_string(), b.to_vec()))
            .collect(),
        world: world.clone(),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (main_id, real) = id_from_env("DEMO_MAIN_ID", STAND_IN_MAIN);
    let (alt_id, _) = id_from_env("DEMO_ALT_ID", STAND_IN_ALT);
    let source = if real { "" } else { " (stand-in IDs)" };

    let tmp = std::env::temp_dir().join(format!("discord-inbox-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let tmp_text = tmp.display().to_string();
    let inbox = tmp.join("Inbox");

    // Config file in the temp dir, read through the bot's real config path.
    let cfg = tmp.join("allowed_ids");
    fs::write(&cfg, format!("# demo allowlist\n{main_id}\n{alt_id}\n")).unwrap();
    // Edition 2021: set_var is safe. Only this process reads it.
    std::env::set_var("DISCORD_INBOX_ALLOWED_IDS_FILE", &cfg);
    let path = allowlist::default_path();
    let allow: Allowlist = Allowlist::load(&path).expect("demo allowlist loads");
    println!(
        "allowlist loaded from {}: {}{source}",
        show(&path, &tmp_text),
        plural(allow.ids().count(), "ID")
    );
    println!();

    let world = Arc::new(World::default());
    let main_msg = demo_msg(
        &world,
        main_id,
        MAIN_CH,
        1427000000000000001,
        "2026-10-13T14:03:22Z",
        "https://example.com/recipe",
        &[],
    );
    let alt_msg = demo_msg(
        &world,
        alt_id,
        ALT_CH,
        1427000000000000002,
        "2026-10-13T14:05:10Z",
        "see attached",
        &[("note.txt", b"hello"), ("bad-photo.jpg", b"jpeg bytes")],
    );
    let third_msg = demo_msg(
        &world,
        THIRD_ID,
        THIRD_CH,
        1427000000000000003,
        "2026-10-13T14:06:00Z",
        "spam",
        &[("x.txt", b"x")],
    );

    // Outcomes are checked, not announced: any wrong one is collected and the demo exits 1.
    let mut problems: Vec<String> = Vec::new();

    println!("== live, CDN down ==");
    world.cdn_down.store(true, Ordering::SeqCst);
    let live = [
        ("main", &main_msg),
        ("alt", &alt_msg),
        ("third", &third_msg),
    ];
    let expected = [Outcome::Saved, Outcome::Pending, Outcome::Ignored];
    for ((label, m), want) in live.iter().zip(expected.iter()) {
        let got = ingest(&allow, &inbox, *m).await;
        println!("{label}: {got:?} -> {}", mark_text(&world.marks(m.id)));
        if got != *want {
            problems.push(format!("{label}: {got:?}, expected {want:?}"));
        }
    }

    println!();
    println!("== next start, CDN back ==");
    world.cdn_down.store(false, Ordering::SeqCst);
    let history = DemoHistory {
        main_id,
        alt_id,
        msgs: vec![main_msg.clone(), alt_msg.clone()],
    };
    let alt_before = world.marks(alt_msg.id);
    catch_up(&allow, &inbox, &history).await;
    let alt_after = world.marks(alt_msg.id);
    println!("alt: {} -> {}", mark_text(&alt_before), mark_text(&alt_after));
    for old in alt_before.iter().filter(|m| !alt_after.contains(m)) {
        println!("  {old} removed from alt's message");
    }
    println!("main: {} (unchanged)", mark_text(&world.marks(main_msg.id)));
    println!("retry list: {} entries", retry_entries(&inbox).len());

    let listing = |dir: &Path| -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    };
    let all = listing(&inbox);
    let names: Vec<String> = all
        .iter()
        .filter(|n| !n.starts_with('.'))
        .cloned()
        .collect();

    // The ignored sender: no download, no reaction, no file, no name, no text.
    if world.downloads(third_msg.id) != 0 || world.reacts(third_msg.id) != 0 {
        problems.push("the ignored sender caused a download or a reaction".into());
    }
    let third_files = all
        .iter()
        .filter(|n| n.contains(&third_msg.id.to_string()))
        .count();
    let stranger_in_text = names.iter().filter(|n| n.ends_with(".md")).any(|n| {
        fs::read_to_string(inbox.join(n))
            .map(|t| t.contains(&THIRD_ID.to_string()))
            .unwrap_or(false)
    });
    if third_files > 0 || stranger_in_text {
        problems.push("a file exists for the ignored sender".into());
    }

    // The two allowlisted DMs: complete notes, one reaction each, cursors at their IDs.
    if !retry_entries(&inbox).is_empty() {
        problems.push("the retry list is not empty".into());
    }
    let main_note = dump::note_path(&inbox, main_msg.received, main_msg.id);
    let alt_note = dump::note_path(&inbox, alt_msg.received, alt_msg.id);
    if !main_note.is_file() {
        problems.push("no note for main".into());
    }
    if !alt_note.is_file() {
        problems.push("no note for alt".into());
    }
    let alt_text = fs::read_to_string(&alt_note).unwrap_or_default();
    if alt_text.contains("pending") || alt_text.matches("sha256: ").count() != 2 {
        problems.push("the alt note is not complete".into());
    }
    if names.len() != 4 {
        problems.push(format!("inbox holds {} files, expected 4", names.len()));
    }
    if !names.iter().any(|n| n.ends_with("_1_note.txt"))
        || !names.iter().any(|n| n.ends_with("_2_bad-photo.jpg"))
    {
        problems.push("the alt attachments are not both on disk".into());
    }
    if world.marks(main_msg.id) != ["✅"] || world.marks(alt_msg.id) != ["✅"] {
        problems.push(format!(
            "marks main={:?} alt={:?}, expected ✅ and ✅",
            world.marks(main_msg.id),
            world.marks(alt_msg.id)
        ));
    }
    if !world.marks(third_msg.id).is_empty() {
        problems.push("the ignored sender got a mark".into());
    }
    if read_cursor(&inbox, MAIN_CH) != Some(main_msg.id) {
        problems.push(format!("main cursor is {:?}", read_cursor(&inbox, MAIN_CH)));
    }
    if read_cursor(&inbox, ALT_CH) != Some(alt_msg.id) {
        problems.push(format!("alt cursor is {:?}", read_cursor(&inbox, ALT_CH)));
    }
    if inbox.join(".cursor-7009").exists() {
        problems.push("a cursor exists for the ignored sender's channel".into());
    }

    // The alt note carries the attachments. Show it whole, with the real IDs masked.
    if alt_note.is_file() {
        let mut pairs = [(main_id, "main"), (alt_id, "alt")];
        // Longest first, so a short ID cannot mask part of a longer one.
        pairs.sort_by_key(|(id, _)| std::cmp::Reverse(id.to_string().len()));
        let mut text = fs::read_to_string(&alt_note).unwrap();
        for (id, label) in pairs {
            text = text.replace(&id.to_string(), label);
        }
        println!();
        println!("--- note for alt (real IDs masked) ---");
        print!("{text}");
        println!("--- end of note ---");
    }

    let notes = names.iter().filter(|n| n.ends_with(".md")).count();
    let files = names.len() - notes;
    println!();
    println!(
        "Inbox {} now holds {} and {}; {} written for the ignored sender.",
        show(&inbox, &tmp_text),
        plural(notes, "note"),
        plural(files, "attachment"),
        if third_files == 0 && !stranger_in_text {
            "nothing"
        } else {
            "FILES"
        }
    );
    println!("(The real target is ~/Documents/Inbox/.)");

    for n in names.iter().filter(|n| !n.ends_with(".md")) {
        let bytes = fs::read(inbox.join(n)).unwrap();
        println!(
            "attachment {n}: {:?}  sha256 {}",
            String::from_utf8_lossy(&bytes),
            sha256_hex(&bytes)
        );
    }

    // The retry fails again: the CDN is still down, so the mark stays on ⏳.
    println!();
    println!("== next start, CDN still down ==");
    world.cdn_down.store(true, Ordering::SeqCst);
    let fourth_msg = demo_msg(
        &world,
        alt_id,
        ALT_CH,
        1427000000000000004,
        "2026-10-13T14:09:45Z",
        "one more scan",
        &[("bad-scan.jpg", b"scan bytes")],
    );
    let first = ingest(&allow, &inbox, &fourth_msg).await;
    println!("fourth: {first:?} -> {}", mark_text(&world.marks(fourth_msg.id)));
    let before_retry = world.marks(fourth_msg.id);
    let history = DemoHistory {
        main_id,
        alt_id,
        msgs: vec![main_msg.clone(), alt_msg.clone(), fourth_msg.clone()],
    };
    catch_up(&allow, &inbox, &history).await;
    let after_retry = world.marks(fourth_msg.id);
    println!(
        "fourth retry: {} -> {}",
        mark_text(&before_retry),
        mark_text(&after_retry)
    );
    if first != Outcome::Pending || after_retry != [PENDING_MARK.to_string()] {
        problems.push(format!(
            "fourth: {first:?} and marks {after_retry:?}, expected Pending and ⏳ to stay"
        ));
    }

    fs::remove_dir_all(&tmp).unwrap();
    if !problems.is_empty() {
        eprintln!("DEMO FAILED: {}", problems.join("; "));
        std::process::exit(1);
    }
    println!("PASS: every outcome checked against the inbox listing.");
}
