//! Demo: drives one gym session through `pip::handle` on a scratch nutrition folder.
//! Usage: `cargo run --example gym_demo -- [path/to/Gym Plan.md] [message ...]` (default plan: examples/gym-plan.md, a
//! made-up plan). The messages after the plan path are the whole script, sent verbatim in order; with none, the default
//! script runs. Example: `-- examples/gym-plan.md gym "15 15 12" skip "15 14" "1 3" "10 10" "30s 30s" today`.
use discord_inbox::allowlist;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use std::fs;
use std::path::PathBuf;

const AUTHOR: u64 = 100000000000000001;

struct Msg {
    id: u64,
    text: String,
    received: String,
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        AUTHOR
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        5001
    }
    fn received(&self) -> String {
        self.received.clone()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        Vec::new()
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Err("no attachments in the demo".into())
    }
    async fn react(&self, emoji: &str) -> Result<(), String> {
        println!("  react {emoji}");
        Ok(())
    }
    async fn unreact(&self, _emoji: &str) -> Result<(), String> {
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        Vec::new()
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        println!("pip> {}", text.replace('\n', "\npip>   "));
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let plan = args.first().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/gym-plan.md"));
    let root = std::env::temp_dir().join(format!("gym-demo-{}", std::process::id()));
    let dir = root.join("Nutrition");
    fs::create_dir_all(&dir).expect("scratch folder");
    let text = fs::read_to_string(&plan).unwrap_or_else(|e| panic!("cannot read {}: {e}", plan.display()));
    fs::write(dir.join("Gym Plan.md"), text).expect("copy plan");
    let allow = allowlist::parse(&format!("{AUTHOR}\n")).expect("allowlist");
    let cfg = Config {
        nutrition_dir: dir.clone(),
        recipes_dir: None,
        low_kcal: 1800,
        kcal_target: 2400,
        model: None,
        model_timeout: std::time::Duration::from_secs(90),
    };
    let default_script = ["gym", "15 15 12", "skip", "15 14", "stop", "today"];
    let script: Vec<String> = if args.len() > 1 { args[1..].to_vec() } else { default_script.iter().map(|s| s.to_string()).collect() };
    for (i, t) in script.iter().enumerate() {
        println!("you> {t}");
        let msg = Msg { id: 900 + i as u64, text: t.clone(), received: format!("2026-10-28T{:02}:{:02}:00+00:00", 18 + i / 60, i % 60) };
        let out = pip::handle(&allow, &cfg, &msg).await;
        println!("  outcome: {}", match out {
            Outcome::Saved => "saved",
            Outcome::Skipped => "skipped",
            Outcome::Failed => "failed",
            _ => "other",
        });
    }
    println!("\n--- 2026-10-28.md ---");
    println!("{}", fs::read_to_string(dir.join("2026-10-28.md")).unwrap_or_else(|_| "(no day note)".into()));
    let _ = fs::remove_dir_all(&root);
}
