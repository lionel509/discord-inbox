//! Offline demo of the warm reply path (#67) for review. Five DMs run through the real `ingest_with`
//! against a temp copy of the vault folder layout. The chat model is a canned stub, every reply is labelled
//! `[canned stub model answer]`, and the receipt verdict is `examples/not_receipt_stub.sh`. OCR is the real
//! local Vision helper on the Hua Lian fixture (`tests/fixtures/receipt_zh.png`). No request leaves this process.
//!
//! The demo exits non-zero if a reply says `Saved to`, says `Not a receipt`, or carries OCR text.
//!
//! Run: `cargo run -q --example warmth_demo`
use discord_inbox::allowlist;
use discord_inbox::chat::Chat;
use discord_inbox::dump;
use discord_inbox::ingest::{forwarded_text, ingest_with, Filer, Inbound, Outcome};
use discord_inbox::vaults::{Vaults, TARGETS};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

const SENDER: u64 = 100000000000000001;
const CHANNEL: u64 = 7002;
const FIRST_ID: u64 = 1430000000000000200;
const LABEL: &str = "[canned stub model answer]";
const RECEIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/receipt_zh.png");
const STUB: &str = concat!("\"", env!("CARGO_MANIFEST_DIR"), "/examples/not_receipt_stub.sh\"");

type Canned = Result<String, String>;

/// The stub model: answers the asks of one DM in order, and keeps every prompt it was sent.
struct StubChat {
    queue: Mutex<VecDeque<Canned>>,
    seen: Mutex<Vec<String>>,
}

impl Chat for StubChat {
    fn ask(&self, _system: &str, user: &str) -> Result<String, String> {
        self.seen.lock().unwrap().push(user.to_string());
        self.queue.lock().unwrap().pop_front().unwrap_or(Err("no canned answer left".into()))
    }
}

/// One DM as the bot sees it. `marks` is the bot's reactions on the message.
struct DemoMsg {
    id: u64,
    received: String,
    text: String,
    attachments: Vec<(String, Vec<u8>)>,
    replies: Mutex<Vec<String>>,
    marks: Mutex<Vec<String>>,
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
        self.received.clone()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.attachments.iter().map(|(n, _)| n.clone()).collect()
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.attachments.get(i).map(|(_, b)| b.clone()).ok_or("no such attachment".into())
    }
    async fn react(&self, e: &str) -> Result<(), String> {
        let mut m = self.marks.lock().unwrap();
        if !m.iter().any(|x| x == e) {
            m.push(e.to_string());
        }
        Ok(())
    }
    async fn unreact(&self, e: &str) -> Result<(), String> {
        self.marks.lock().unwrap().retain(|x| x != e);
        Ok(())
    }
    fn own_reactions(&self) -> Vec<String> {
        Vec::new()
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        self.replies.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

fn layout(root: &Path) {
    fs::create_dir_all(root.join("Inbox")).unwrap();
    for t in TARGETS.iter() {
        fs::create_dir_all(root.join(t.folder)).unwrap();
    }
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n")
}

struct Case {
    /// What is shown as `DM:`.
    shown: &'static str,
    text: String,
    files: Vec<(String, Vec<u8>)>,
    at: &'static str,
    canned: Vec<Canned>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    // The canned verdict command. Set in-process so nothing reads ~/.claude or an OpenRouter key.
    std::env::set_var("DISCORD_INBOX_INSIGHT", STUB);
    let root: PathBuf = std::env::temp_dir().join(format!("di-warmth-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    layout(&root);
    let inbox = root.join("Inbox");

    let stub = Arc::new(StubChat {
        queue: Mutex::new(VecDeque::new()),
        seen: Mutex::new(Vec::new()),
    });
    let filer = Filer {
        chat: stub.clone(),
        vaults: Vaults::new(root.clone()),
        last_notice: Default::default(),
        window: Default::default(),
    };
    let allow = allowlist::parse(&format!("{SENDER}\n")).expect("demo allowlist");

    let receipt = fs::read(RECEIPT).expect("receipt fixture");
    let ocr_lines: Vec<String> = discord_inbox::ocr::read_all(0, &[("receipt_zh.png".to_string(), Some(receipt.clone()))])
        .await
        .into_iter()
        .flat_map(|(_, t)| t.lines().map(|l| l.trim().to_string()).collect::<Vec<_>>())
        .filter(|l| l.chars().count() >= 3)
        .collect();

    let cases = vec![
        Case {
            shown: "(no text) receipt_zh.png",
            text: String::new(),
            files: vec![("receipt_zh.png".to_string(), receipt.clone())],
            at: "12:01:05",
            canned: vec![
                Ok("FOLDER: State Street\nTITLE: Hua Lian grocery receipt\nTAGS: groceries\nREPLY:\nGot your Hua Lian receipt: milk and bread. Filed it with your spending.".into()),
            ],
        },
        Case {
            shown: "It comes with the bottle tho",
            text: "It comes with the bottle tho".into(),
            files: Vec::new(),
            at: "12:02:10",
            canned: vec![Ok("FOLDER: Chat\nTITLE: \nTAGS: \nREPLY:\nOh nice, that one comes with the bottle.".into())],
        },
        Case {
            shown: "Which one should i get",
            text: "Which one should i get".into(),
            files: Vec::new(),
            at: "12:04:00",
            canned: vec![
                Ok("FOLDER: Answer\nTITLE: \nTAGS: \nREPLY:\nLet me look.".into()),
                Ok("Get the Terra one. It comes with the bottle.".into()),
            ],
        },
        Case {
            shown: "(forward) IPI: the lab is closed Friday",
            text: forwarded_text("", &["IPI: the lab is closed Friday".to_string()]),
            files: Vec::new(),
            at: "12:06:00",
            canned: vec![Ok("FOLDER: Inbox\nTITLE: Lab closed Friday\nTAGS: school\nREPLY:\nThanks for passing that along.".into())],
        },
        Case {
            shown: "with the ingredients in my pantry what can i make with cod",
            text: "with the ingredients in my pantry what can i make with cod".into(),
            files: Vec::new(),
            at: "12:08:30",
            canned: vec![
                Ok("FOLDER: Answer\nTITLE: \nTAGS: \nREPLY:\nLooking.".into()),
                Ok("Cod with soy sauce, from what's in your pantry.".into()),
            ],
        },
    ];

    println!("Real vs canned: the prompt, the window, the reactions and the filing come from the code under test; the reply wording is a stub ({LABEL}).");
    println!("Legend: \"filed to\" = the note went into a vault folder. \"kept in Inbox\" = nothing went to a vault folder (the answers that file nothing); the Inbox copy is still kept.");
    println!("Bot log lines go to stderr, not this output: run with 2>/dev/null to hide them.");

    let mut pass = true;
    for (k, c) in cases.into_iter().enumerate() {
        let n = k + 1;
        let id = FIRST_ID + n as u64;
        let m = DemoMsg {
            id,
            received: format!("2026-10-14T{}Z", c.at),
            text: c.text.clone(),
            attachments: c.files.clone(),
            replies: Mutex::new(Vec::new()),
            marks: Mutex::new(Vec::new()),
        };
        *stub.queue.lock().unwrap() = c.canned.into_iter().collect();
        let before = stub.seen.lock().unwrap().len();
        let outcome = ingest_with(&allow, &inbox, &m, &filer).await;
        pass &= outcome == Outcome::Saved;

        println!("\n=== DM ({n}) ===");
        println!("DM: {}", c.shown);
        let seen = stub.seen.lock().unwrap().clone();
        match seen.get(before) {
            Some(first) => {
                let lines: Vec<&str> = first.lines().collect();
                let tail = &lines[lines.len().saturating_sub(12)..];
                println!("model saw (last 12 lines of the prompt):\n{}", indent(&tail.join("\n")));
            }
            None => println!("model saw: nothing (the model was not asked)"),
        }
        let replies = m.replies.lock().unwrap().clone();
        let marks = m.marks.lock().unwrap().clone();
        match replies.first() {
            Some(r) => {
                println!("reply: {LABEL}");
                println!("{}", indent(r));
                if r.contains("Saved to") || r.contains("Not a receipt") || ocr_lines.iter().any(|l| r.contains(l.as_str())) {
                    println!("  LEAK: reply carries a banned line");
                    pass = false;
                }
            }
            None => println!("reply: none"),
        }
        println!("reactions: {}", if marks.is_empty() { "none".to_string() } else { marks.join(" ") });
        let where_ = match dump::filed_to(&inbox, id) {
            Some(dest) => format!("filed to {}", rel(&root, Path::new(&dest))),
            None => format!("kept in Inbox: {}", rel(&root, &dump::note_path(&inbox, &m.received, id))),
        };
        println!("note: {where_}");
        println!("outcome: {outcome:?}");
    }

    let _ = fs::remove_dir_all(&root);
    println!("\n{}", if pass { "PASS" } else { "FAIL" });
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
