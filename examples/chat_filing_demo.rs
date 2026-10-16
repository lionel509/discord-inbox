//! Offline demo of chat replies and filing (#31) for review. Six DMs run through the real `ingest_with`
//! against a temp copy of the vault folder layout. The chat model is a canned stub, and the receipt verdict
//! is `examples/chat_filing_insight_stub.sh`. OCR is the real local Vision helper on the Hua Lian fixture
//! (`tests/fixtures/receipt_zh.png`). No request leaves this process, and every reply is labelled
//! `[canned stub model answer]`.
//!
//! Run: `cargo run -q --example chat_filing_demo`
use discord_inbox::allowlist;
use discord_inbox::chat::{self, Chat};
use discord_inbox::dump;
use discord_inbox::ingest::{ingest_with, Filer, Inbound, Outcome};
use discord_inbox::insight::mask_cards;
use discord_inbox::vaults::{self, Vaults, TARGETS};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

const SENDER: u64 = 100000000000000001;
const CHANNEL: u64 = 7001;
const FIRST_ID: u64 = 1430000000000000100;
const LABEL: &str = "[canned stub model answer]";
const RECEIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/receipt_zh.png");

type Canned = Result<String, String>;

/// The stub model: returns the canned answer set for the next ask and keeps every prompt it was sent.
struct StubChat {
    next: Mutex<Option<Canned>>,
    seen: Mutex<Vec<String>>,
}

impl Chat for StubChat {
    fn ask(&self, _system: &str, user: &str) -> Result<String, String> {
        self.seen.lock().unwrap().push(user.to_string());
        self.next.lock().unwrap().take().unwrap_or(Err("no canned answer set".into()))
    }
}

/// One DM as the bot sees it. `marks` is what the bot's reactions currently are on the message.
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
    fs::create_dir_all(root.join("Private")).unwrap();
    fs::write(root.join("Private/existing note.md"), "private, never touched\n").unwrap();
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

fn head(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n")
}

struct Case {
    text: &'static str,
    at: &'static str,
    receipt: bool,
    canned: Canned,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    // The canned verdict command. Set in-process so nothing reads ~/.claude or an OpenRouter key.
    std::env::set_var(
        "DISCORD_INBOX_INSIGHT",
        concat!("\"", env!("CARGO_MANIFEST_DIR"), "/examples/chat_filing_insight_stub.sh\""),
    );
    let root: PathBuf = std::env::temp_dir().join(format!("di-filing-demo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    layout(&root);
    let inbox = root.join("Inbox");

    let stub = Arc::new(StubChat {
        next: Mutex::new(None),
        seen: Mutex::new(Vec::new()),
    });
    let filer = Filer {
        chat: stub.clone(),
        vaults: Vaults::new(root.clone()),
        last_notice: Default::default(),
        window: Default::default(),
    };
    let allow = allowlist::parse(&format!("{SENDER}\n")).expect("demo allowlist");

    let cases = vec![
        Case {
            text: "idea: a 3D-printed cable clip for my desk",
            at: "12:01:05",
            receipt: false,
            canned: Ok("FOLDER: Citadel/Ideas\nTITLE: 3D-printed desk cable clip\nTAGS: 3d-printing, desk, idea\nREPLY:\nFiled as a 3D printing idea. Measure your desk edge before you print it.".into()),
        },
        Case {
            text: "recipe: tomato egg stir fry, 3 eggs 2 tomatoes",
            at: "12:04:40",
            receipt: false,
            canned: Ok("FOLDER: Fidelity/Want to Make\nTITLE: Tomato egg stir fry\nTAGS: recipe, eggs, tomato\nREPLY:\nSaved as a recipe to make. Scramble the eggs first, then add the tomatoes.".into()),
        },
        Case {
            text: "had dinner with Kevin, feeling sick",
            at: "12:09:12",
            receipt: false,
            canned: Ok("FOLDER: Private\nTITLE: Dinner with Kevin\nTAGS: health, friends\nREPLY:\nSorry you feel sick after dinner. Kept it in your Inbox.".into()),
        },
        Case {
            text: "receipt from Hua Lian, for the groceries",
            at: "12:15:03",
            receipt: true,
            canned: Ok("FOLDER: State Street\nTITLE: Hua Lian grocery receipt\nTAGS: receipt, groceries\nREPLY:\nFiled as a grocery receipt.".into()),
        },
        Case {
            text: "idea: a desk lamp with a wooden base",
            at: "12:21:30",
            receipt: false,
            canned: Err("http 503".into()),
        },
        Case {
            text: "paid card 4111 1111 1111 1111 for printer filament, log it",
            at: "12:26:55",
            receipt: false,
            canned: Ok("FOLDER: State Street\nTITLE: Printer filament purchase\nTAGS: purchase, printing\nREPLY:\nLogged as a purchase.".into()),
        },
    ];

    let receipt_bytes = fs::read(RECEIPT).expect("receipt fixture");
    let mut all_saved = true;
    let mut note_for_3: Option<String> = None;
    let mut ids: Vec<u64> = Vec::new();
    for (k, c) in cases.iter().enumerate() {
        let n = k + 1;
        let id = FIRST_ID + n as u64;
        ids.push(id);
        let m = DemoMsg {
            id,
            received: format!("2026-10-14T{}Z", c.at),
            text: c.text.to_string(),
            attachments: if c.receipt {
                vec![("receipt_zh.png".to_string(), receipt_bytes.clone())]
            } else {
                Vec::new()
            },
            replies: Mutex::new(Vec::new()),
            marks: Mutex::new(Vec::new()),
        };
        *stub.next.lock().unwrap() = Some(c.canned.clone());
        let before = stub.seen.lock().unwrap().len();
        let outcome = ingest_with(&allow, &inbox, &m, &filer).await;
        all_saved &= outcome == Outcome::Saved;

        println!("\n=== ({n}) DM: {} ===", c.text);
        let sent = stub.seen.lock().unwrap().len() > before;
        if sent {
            let input = stub.seen.lock().unwrap().last().cloned().unwrap_or_default();
            println!("  model input (after masking):\n{}", indent(&head(&input, 400)));
        } else {
            println!("  model input: none (the model was not asked)");
        }
        let replies = m.replies.lock().unwrap().clone();
        let marks = m.marks.lock().unwrap().clone();
        println!("  Discord gets:");
        if replies.is_empty() {
            println!("    marks only: {}", marks.join(" "));
        } else {
            println!("    {LABEL}");
            println!("{}", indent(&replies[0]));
        }
        let (where_, path) = match dump::filed_to(&inbox, id) {
            Some(dest) => (format!("filed to {}", rel(&root, Path::new(&dest))), PathBuf::from(dest)),
            None => {
                let p = dump::note_path(&inbox, &m.received, id);
                (format!("kept in Inbox: {}", rel(&root, &p)), p)
            }
        };
        println!("  note: {where_}");
        if n == 3 {
            note_for_3 = Some(rel(&root, &path));
        }
        if let Ok(text) = fs::read_to_string(&path) {
            println!("{}", indent(&text.lines().take(20).collect::<Vec<_>>().join("\n")));
        }
        println!("  outcome: {outcome:?}");
    }

    println!("\n=== request body for DM (1) (built by chat::request_body) ===");
    let user = vaults::user_prompt(&mask_cards(cases[0].text), &[]);
    let body = chat::request_body(&vaults::system_prompt(), &user);
    println!("{}", serde_json::to_string_pretty(&body).unwrap());

    println!("\n=== tree ===");
    print_tree(&root, &root);
    let private = fs::read_dir(root.join("Private")).unwrap().count();
    println!("\nPrivate files: {private} (only the existing note)");
    let model_ok = body["model"] == chat::MODEL && body["provider"]["zdr"] == true;
    let in_inbox = note_for_3.as_deref().is_some_and(|p| p.starts_with("Inbox/"));
    let pass = all_saved && private == 1 && model_ok && in_inbox && ids.len() == 6;
    let _ = fs::remove_dir_all(&root);
    println!("{}", if pass { "PASS" } else { "FAIL" });
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_tree(dir: &Path, base: &Path) {
    let mut items: Vec<PathBuf> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    items.sort();
    for p in items {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        println!("  {}", rel(base, &p));
        if p.is_dir() {
            print_tree(&p, base);
        }
    }
}
