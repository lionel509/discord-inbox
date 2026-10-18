//! Stormy's small-task demo (#84): one message through the helper on the vault root from `DISCORD_INBOX_VAULTS_DIR`.
//! Point it at a scratch copy, never the real finance vault: `sh examples/task_demo.sh "<message>"` does that for you.
//!
//! Run: `DISCORD_INBOX_VAULTS_DIR=<scratch> cargo run --example task_demo -- "<message>"`
use discord_inbox::task;
use discord_inbox::vaults::Vaults;
use std::process::ExitCode;

fn main() -> ExitCode {
    let text = std::env::args().nth(1).unwrap_or_default();
    let root = Vaults::default_root();
    let inbox = root.join("Discord Inbox");
    let settings: serde_json::Value = serde_json::from_str(&task::settings_json(&root, &inbox)).unwrap();
    println!("root: {}", root.display());
    println!("settings:\n{}", serde_json::to_string_pretty(&settings).unwrap());
    println!("ledger: {}", task::ledger_cmd(&root));

    let before = task::snapshot(&root);
    let Some(reply) = task::run(&root, &inbox, &text, task::TIMEOUT) else {
        println!("no reply (the helper failed, timed out, or the claude binary is missing)");
        return ExitCode::from(1);
    };
    println!("reply:\n{reply}");
    let title: String = text.lines().next().unwrap_or("").chars().take(60).collect();
    for (vault, sha, paths) in task::commit_changes(&root, &before, &format!("stormy: {title}")) {
        println!("📝 {vault} @ {sha}: {}", paths.join(", "));
    }
    ExitCode::SUCCESS
}
