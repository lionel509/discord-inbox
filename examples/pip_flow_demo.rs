//! Pip flow demo (PR head only): a recipe name, then undo and fix, sent as DMs through the real
//! handler `pip::handle`. Every reply is printed, as Discord would show it, and the day note is
//! printed at the end. scripts/demo-pip-v2.sh sets PIP_NUTRITION_DIR and PIP_RECIPES_DIR to scratch
//! folders under a temp dir; nothing here touches Discord or a token.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use std::fs;

const USER: u64 = 100000000000000001;
const CH: u64 = 5001;

/// One DM. Its reply is printed, so the demo shows what Discord would have received.
struct Msg {
    id: u64,
    received: &'static str,
    text: &'static str,
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        USER
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        CH
    }
    fn received(&self) -> String {
        self.received.to_string()
    }
    fn content(&self) -> String {
        self.text.to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        Vec::new()
    }
    fn live(&self) -> bool {
        true
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Err("no attachments in the demo".to_string())
    }
    async fn react(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    async fn unreact(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        Vec::new()
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        println!("  reply:");
        for line in text.lines() {
            println!("    {line}");
        }
        Ok(())
    }
}

/// Sends one DM through the real handler and prints the outcome if it was not a saved meal.
async fn send(allow: &allowlist::Allowlist, cfg: &Config, id: u64, received: &'static str, text: &'static str) {
    println!("> {text}   (msg {id}, {received})");
    let m = Msg { id, received, text };
    match pip::handle(allow, cfg, &m).await {
        Outcome::Saved => {}
        other => println!("  outcome for {id}: {other:?}"),
    }
    println!();
}

#[tokio::main]
async fn main() {
    let cfg = Config::from_env().expect("demo-pip-v2.sh sets the scratch folders");
    let allow = allowlist::parse(&format!("{USER}\n")).expect("one id");

    send(&allow, &cfg, 9301, "2026-10-26T12:15:00-04:00", "2 x salmon bowl").await;
    send(&allow, &cfg, 9302, "2026-10-26T12:20:00-04:00", "salmon 150 g").await;
    send(&allow, &cfg, 9303, "2026-10-26T12:25:00-04:00", "undo").await;
    send(&allow, &cfg, 9304, "2026-10-26T12:30:00-04:00", "salmon 200 g").await;
    send(&allow, &cfg, 9305, "2026-10-26T12:35:00-04:00", "fix 2 lb 4 oz salmon").await;

    let day = daylog::day_key("2026-10-26T12:15:00-04:00");
    let note = daylog::note_path(&cfg.nutrition_dir, day);
    println!("Day note after the flow ({}):", note.display());
    match fs::read_to_string(&note) {
        Ok(text) => {
            for line in text.lines() {
                println!("  {line}");
            }
        }
        Err(e) => println!("  (no note written: {e})"),
    }
}
