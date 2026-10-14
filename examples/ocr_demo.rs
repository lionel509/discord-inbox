//! Offline OCR demo: each image path becomes one DM with one attachment, run through the real
//! `ingest::ingest`, then the note it wrote is printed. No Discord, no token.
//!
//! Run: `DISCORD_INBOX_DIR=<scratch> cargo run --release --example ocr_demo -- <image>...`
//! `DISCORD_INBOX_DIR` is required, so the demo never writes to `~/Documents/Inbox`.
//! Optional: `DISCORD_INBOX_BUDGET` (budget note template) and `DISCORD_INBOX_INSIGHT=` (empty skips the local model).
//! Use only synthetic images here: the notes are printed to stdout.
use discord_inbox::allowlist;
use discord_inbox::dump::{self, note_path};
use discord_inbox::ingest::{ingest, Inbound, Outcome};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

const SENDER: u64 = 100000000000000001;
const CHANNEL: u64 = 7001;
const FIRST_ID: u64 = 1430000000000000000;
const RECEIVED: &str = "2026-10-13T14:03:22Z";

/// One DM carrying one attachment: the image read from disk.
struct DemoMsg {
    id: u64,
    file: String,
    bytes: Vec<u8>,
}

impl Inbound for DemoMsg {
    fn author_id(&self) -> u64 {
        SENDER
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        CHANNEL
    }
    fn received(&self) -> String {
        RECEIVED.to_string()
    }
    fn content(&self) -> String {
        "receipt".to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        vec![self.file.clone()]
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(self.bytes.clone())
    }
    async fn react(&self) -> Result<(), String> {
        Ok(())
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        println!("-- reply --\n{text}");
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("usage: ocr_demo <image>...");
        return ExitCode::from(2);
    }
    if std::env::var_os("DISCORD_INBOX_DIR").is_none() {
        eprintln!("set DISCORD_INBOX_DIR to a scratch directory");
        return ExitCode::from(2);
    }
    let inbox = dump::inbox_dir();
    fs::create_dir_all(&inbox).expect("create scratch inbox");
    let allow = allowlist::parse(&format!("{SENDER}\n")).expect("demo allowlist");

    let mut ok = true;
    for (k, path) in paths.iter().enumerate() {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        };
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let msg = DemoMsg {
            id: FIRST_ID + k as u64,
            file,
            bytes,
        };
        let outcome = ingest(&allow, &inbox, &msg).await;
        if outcome != Outcome::Saved {
            ok = false;
        }
        println!("== {}: {outcome:?}", path.display());
        match fs::read_to_string(note_path(&inbox, RECEIVED, msg.id)) {
            Ok(text) => print!("{text}"),
            Err(e) => println!("(no note: {e})"),
        }
        println!();
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
