// Demo for #95: runs the check-off against the calendar stub with a scripted model, and prints each reply. Real Calendar.app is never called. Run through examples/calendar_demo.sh.
use discord_inbox::calendar::{self, Finish};
use discord_inbox::chat::Chat;
use std::collections::VecDeque;
use std::sync::Mutex;

/// Answers from a script and shows what the model was sent.
struct Scripted(Mutex<VecDeque<&'static str>>);

impl Chat for Scripted {
    fn ask(&self, _system: &str, user: &str) -> Result<String, String> {
        let reply = self.0.lock().unwrap().pop_front().unwrap_or("NONE");
        for line in user.lines() {
            println!("    | {line}");
        }
        println!("    model replies: {reply}");
        Ok(reply.to_string())
    }
}

fn main() {
    let Some(cal) = calendar::Cal::from_env() else {
        eprintln!("calendar_demo: no calendars file (set DISCORD_INBOX_CALENDARS_FILE)");
        std::process::exit(1);
    };
    let chat = Scripted(Mutex::new(VecDeque::from(["#1", "NONE", "#1 #2"])));
    for task in ["math hw 3", "the seltzer run", "phys"] {
        println!("DM: done with {task}");
        match calendar::finish(&chat, &cal, task, "2026-10-19") {
            Ok(Finish::Done(text)) | Ok(Finish::Ask(text)) => println!("reply: {text}"),
            Ok(Finish::NoMatch) => println!("reply: (no match, falls through to chat)"),
            Err(e) => println!("reply: (error: {e})"),
        }
        println!();
    }
}
