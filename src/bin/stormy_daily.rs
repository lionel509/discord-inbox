// Dry run for the daily brief (#135). Never opens Discord, never takes Inbox/.lock, never sends a DM.
// Usage: stormy-daily morning | night | reply "<text>"
use chrono::Local;
use discord_inbox::calendar::Cal;
use discord_inbox::chat::{self, Chat, NoChat, OpenRouter};
use discord_inbox::daily::{self, Deps, Slot};
use discord_inbox::redact;
use discord_inbox::todo;
use discord_inbox::vaults::Vaults;
use discord_inbox::weather;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

/// Top-level files of the to-do folder, so the reply run can show what it changed.
fn snapshot(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Ok(text) = fs::read_to_string(e.path()) {
                out.insert(name, text);
            }
        }
    }
    out
}

fn usage() -> ExitCode {
    eprintln!("usage: stormy-daily morning | night | reply \"<text>\"");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let chat: Arc<dyn Chat> = match OpenRouter::from_key_file(&chat::key_path()) {
        Some(c) => Arc::new(c),
        None => Arc::new(NoChat),
    };
    let deps = Deps {
        todo_dir: Vaults::default_root().join(todo::DIR),
        cal: Cal::from_env(),
        chat,
    };
    let now = Local::now().naive_local();
    // DISCORD_INBOX_TODAY pins the date for dry runs; the bot does not read it.
    let today = std::env::var("DISCORD_INBOX_TODAY").ok().and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()).unwrap_or(now.date());

    match args.first().map(String::as_str) {
        Some(which @ ("morning" | "night")) => {
            let slot = if which == "morning" { Slot::Morning } else { Slot::Night };
            let (events, warn) = daily::read_events(deps.cal.as_ref());
            let facts = match slot {
                Slot::Morning => {
                    let w = weather::fetch();
                    daily::morning_facts(&deps.todo_dir, &events, w.as_ref(), today)
                }
                Slot::Night => daily::night_facts(&deps.todo_dir, &events, today),
            };
            let facts = if warn.is_some() { daily::unread(&facts) } else { facts };
            // A failed read sends the unread facts as they are: the model is not asked about a calendar it never saw (#138 MF-E).
            let voiced = if warn.is_some() { facts.clone() } else { daily::voice(deps.chat.as_ref(), slot, &facts) };
            let dm = redact::sensitive(&voiced);
            let dm = match warn {
                Some(w) => format!("{w}\n{dm}"),
                None => dm,
            };
            let dm: String = dm.chars().take(daily::MAX_CHARS).collect();
            println!("FACTS:\n{facts}\n");
            println!("DM:\n{dm}");
            ExitCode::SUCCESS
        }
        Some("reply") => {
            let Some(text) = args.get(1) else {
                return usage();
            };
            let before = snapshot(&deps.todo_dir);
            match daily::answer(&deps, today, text, None) {
                Ok(Some(reply)) => println!("REPLY:\n{reply}\n"),
                Ok(None) => println!("REPLY:\n(no check-in: the message would be filed as usual)\n"),
                Err(e) => {
                    eprintln!("stormy-daily: reply failed: {e}");
                    return ExitCode::FAILURE;
                }
            }
            let after = snapshot(&deps.todo_dir);
            println!("FILES CHANGED:");
            let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
            names.sort();
            names.dedup();
            let mut any = false;
            for name in names {
                if before.get(name) != after.get(name) {
                    any = true;
                    println!("--- {name}");
                    let old = before.get(name).map(String::as_str).unwrap_or("");
                    let new = after.get(name).map(String::as_str).unwrap_or("");
                    for line in old.lines().filter(|l| !new.lines().any(|n| n == *l)) {
                        println!("- {line}");
                    }
                    for line in new.lines().filter(|l| !old.lines().any(|o| o == *l)) {
                        println!("+ {line}");
                    }
                }
            }
            if !any {
                println!("(none)");
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
