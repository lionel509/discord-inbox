//! Offline demo of the verdict chain. `chain_demo model` prints which model `$DISCORD_INBOX_INSIGHT` picks;
//! `chain_demo ask` runs `ask_model` on stdin and prints the verdict as the reply header shows it.
use discord_inbox::insight::{ask_model, model, reply_text, TIMEOUT};
use std::io::Read;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "model" {
        println!("{:?}", model());
        return;
    }
    let mut ocr = String::new();
    std::io::stdin().read_to_string(&mut ocr).unwrap();
    let insight = ask_model(&model(), &ocr, TIMEOUT);
    let header = reply_text(&insight.into_iter().collect::<Vec<_>>(), None);
    println!("{}", header.lines().next().unwrap_or("(no verdict)"));
}
