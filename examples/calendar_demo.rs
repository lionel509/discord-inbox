// Demo for #95: the done-with-task check-off against the calendar stub, with the live model picking. Real Calendar.app is never called. Run through examples/calendar_demo.sh.
use discord_inbox::calendar::{self, Event, Finish};
use discord_inbox::chat::{self, Chat, OpenRouter};

/// Shows what the live model was sent and what it answered.
struct Echo(OpenRouter);

impl Chat for Echo {
    fn ask(&self, system: &str, user: &str) -> Result<String, String> {
        for line in user.lines() {
            println!("    | {line}");
        }
        let reply = self.0.ask(system, user)?;
        println!("    model replies: {reply}");
        Ok(reply)
    }
}

fn show(evs: &[Event]) {
    for e in evs {
        println!("    {} | {} | {} | {}", e.uid, e.calendar, e.date, e.summary);
    }
}

/// Says why each listed event was not offered to the model. Only two things drop one: already checked off, or past the cap.
fn explain(evs: &[Event], today: &str) {
    let open = calendar::candidates(evs, today);
    let offered = |e: &Event| open.iter().any(|o| o.uid == e.uid && o.calendar == e.calendar);
    println!("    calendar list: {} events in 2 calendars, {} offered to the model", evs.len(), open.len());
    for e in evs.iter().filter(|e| !offered(e)) {
        let why = if e.done() { "already checked off" } else { "past the cap" };
        println!("    left out: {} {} {}: {} ({why})", e.uid, e.date, e.calendar, e.summary);
    }
}

fn main() {
    let Some(cal) = calendar::Cal::from_env() else {
        eprintln!("calendar_demo: no calendars file (set DISCORD_INBOX_CALENDARS_FILE)");
        std::process::exit(1);
    };
    let Some(live) = OpenRouter::from_key_file(&chat::key_path()) else {
        eprintln!("calendar_demo: no OpenRouter key at {} (no scripted fallback)", chat::key_path().display());
        std::process::exit(1);
    };
    let chat = Echo(live);
    let today = "2026-10-19";
    for dm in ["done with math hw 3", "done with the seltzer run", "done with phys", "submitted lab 2"] {
        println!("DM: {dm}");
        let Some(task) = calendar::wanted(dm) else {
            println!("trigger: not a finish message");
            println!();
            continue;
        };
        println!("trigger: task \"{task}\"");
        let before = match calendar::list(&cal) {
            Ok(v) => v,
            Err(e) => {
                println!("reply: (error: {e})\n");
                continue;
            }
        };
        println!("  before:");
        show(&before);
        explain(&before, today);
        match calendar::finish(&chat, &cal, &task, today) {
            Ok(Finish::Done(text)) | Ok(Finish::Ask(text)) => println!("reply: {text}"),
            Ok(Finish::NoMatch) => println!("reply: (no match, falls through to chat)"),
            Err(e) => println!("reply: (error: {e})"),
        }
        if let Ok(after) = calendar::list(&cal) {
            println!("  after:");
            show(&after);
            for b in &before {
                if let Some(a) = after.iter().find(|a| a.uid == b.uid && a.calendar == b.calendar) {
                    if a.summary != b.summary {
                        println!("  title changed: {} -> {}", b.summary, a.summary);
                    }
                }
            }
        }
        println!();
    }
}
