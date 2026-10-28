//! Pip meal-reader demo (#130): a typed meal and, when photo paths are given, a photo-only DM, sent through the real
//! handler `pip::handle` with the real model helper. Usage: `pip_model_demo [photo.png ...]`. Set PIP_NUTRITION_DIR
//! to a scratch folder and PIP_MODEL to the helper (scripts/pip-model.sh) first; see Config::from_env. Every reply is
//! printed as Discord would show it, then the day note. Nothing here touches Discord or a token.
use discord_inbox::allowlist;
use discord_inbox::daylog;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use std::fs;
use std::path::PathBuf;

const USER: u64 = 100000000000000001;
const CH: u64 = 5001;

/// One DM. Its photos are read from disk when the handler asks for them.
struct Msg {
    id: u64,
    received: &'static str,
    text: &'static str,
    photos: Vec<PathBuf>,
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
        self.photos.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect()
    }
    fn live(&self) -> bool {
        true
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        let path = self.photos.get(i).ok_or("no such attachment")?;
        fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
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
async fn send(allow: &allowlist::Allowlist, cfg: &Config, id: u64, received: &'static str, text: &'static str, photos: Vec<PathBuf>) {
    let shown = if photos.is_empty() { text.to_string() } else { format!("{text} [{} photo(s)]", photos.len()) };
    println!("> {shown}   (msg {id}, {received})");
    let m = Msg { id, received, text, photos };
    match pip::handle(allow, cfg, &m).await {
        Outcome::Saved => {}
        other => println!("  outcome for {id}: {other:?}"),
    }
    println!();
}

#[tokio::main]
async fn main() {
    let photos: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let cfg = Config::from_env().expect("set PIP_NUTRITION_DIR and PIP_MODEL (an absolute path)");
    let allow = allowlist::parse(&format!("{USER}\n")).expect("one id");

    send(&allow, &cfg, 9401, "2026-10-27T19:00:00-04:00", "grilled salmon 100g, cabbage slaw", Vec::new()).await;
    if !photos.is_empty() {
        send(&allow, &cfg, 9402, "2026-10-27T19:10:00-04:00", "", photos).await;
    }

    let day = daylog::day_key("2026-10-27T19:00:00-04:00");
    let note = daylog::note_path(&cfg.nutrition_dir, day);
    println!("Day note after the demo ({}):", note.display());
    match fs::read_to_string(&note) {
        Ok(text) => {
            for line in text.lines() {
                println!("  {line}");
            }
        }
        Err(e) => println!("  (no note written: {e})"),
    }
}
