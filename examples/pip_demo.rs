//! Pip demo: real input through the real handler and the real replay path, into the scratch folders
//! that examples/pip_demo.sh sets (PIP_NUTRITION_DIR, PIP_RECIPES_DIR). Never the real vault.
use discord_inbox::allowlist;
use discord_inbox::ingest::{catch_up_each, History, Inbound, Outcome};
use discord_inbox::pip::{self, Config, PipHandler};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const USER: u64 = 100000000000000001;
const CH: u64 = 5001;

#[derive(Clone)]
struct Spec {
    id: u64,
    received: &'static str,
    text: &'static str,
    photo: bool,
}

/// One DM. Its reply is printed, so the demo shows what Discord would have received.
struct Msg {
    spec: Spec,
    live: bool,
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        USER
    }
    fn msg_id(&self) -> u64 {
        self.spec.id
    }
    fn channel_id(&self) -> u64 {
        CH
    }
    fn received(&self) -> String {
        self.spec.received.to_string()
    }
    fn content(&self) -> String {
        self.spec.text.to_string()
    }
    fn attachment_names(&self) -> Vec<String> {
        if self.spec.photo {
            vec!["IMG_1.JPG".to_string()]
        } else {
            Vec::new()
        }
    }
    fn live(&self) -> bool {
        self.live
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(b"not really a jpeg".to_vec())
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
        println!("  reply to {}:", self.spec.id);
        for line in text.lines() {
            println!("    {line}");
        }
        Ok(())
    }
}

/// Discord history for the replay: the DMs that arrived while the laptop was off.
struct Replay {
    msgs: Mutex<Vec<Spec>>,
}

impl History for Replay {
    type Msg = Msg;
    async fn dm_channel(&self, _user_id: u64) -> Result<u64, String> {
        Ok(CH)
    }
    async fn page(&self, _channel: u64, after: Option<u64>) -> Result<Vec<Msg>, String> {
        Ok(self
            .msgs
            .lock()
            .unwrap()
            .iter()
            .filter(|s| after.is_none_or(|a| s.id > a))
            .map(|s| Msg { spec: s.clone(), live: false })
            .collect())
    }
    async fn fetch(&self, _channel: u64, msg_id: u64) -> Result<Msg, String> {
        self.msgs
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == msg_id)
            .map(|s| Msg { spec: s.clone(), live: false })
            .ok_or_else(|| "gone".to_string())
    }
}

fn show_dir(label: &str, dir: &Path) {
    println!("{label}:");
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|rd| rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    for n in names {
        println!("  {n}");
    }
}

/// One DM through the real handler; a non-saved outcome is printed.
async fn send(allow: &allowlist::Allowlist, cfg: &Config, id: u64, received: &'static str, text: &'static str) {
    let m = Msg { spec: Spec { id, received, text, photo: false }, live: true };
    match pip::handle(allow, cfg, &m).await {
        Outcome::Saved => {}
        other => println!("  outcome for {id}: {other:?}"),
    }
}

/// Inserts `add` on the line after the first line `at` accepts.
fn insert_after(text: &str, at: impl Fn(&str) -> bool, add: &str) -> String {
    let mut out = String::new();
    let mut done = false;
    for line in text.split_inclusive('\n') {
        out.push_str(line);
        if !done && at(line.trim_end()) {
            out.push_str(add);
            done = true;
        }
    }
    assert!(done, "anchor line not found");
    out
}

/// How many of `his` appear exactly once as whole lines of `note`.
fn kept(note: &str, his: &[&str]) -> usize {
    his.iter().filter(|h| note.lines().filter(|l| l.trim_end() == **h).count() == 1).count()
}

/// The lines he typed by hand. The `---` fence is left out: it appears twice by design.
const HIS: [&str; 5] = [
    "mood: tired",
    "how full: very",
    "Felt bloated after lunch.",
    "ate late, ignore the flag",
    "skipped the cabbage",
];

#[tokio::main]
async fn main() {
    let cfg = Config::from_env().expect("pip_demo.sh sets the scratch folders");
    let allow = allowlist::parse(&format!("{USER}\n")).expect("one id");
    let nutrition = cfg.nutrition_dir.clone();
    let recipes = cfg.recipes_dir.clone().expect("pip_demo.sh sets PIP_RECIPES_DIR");

    println!("== (a) live DMs: the parser and day writer on typed input ==");
    let typed = [
        (9001, "2026-10-26T12:31:00-04:00", "100g salmon, 1/2 cup dry rice, cabbage + 1 tbsp oil", false),
        (9002, "2026-10-26T18:05:00-04:00", "1 serving salmon bowl, 150g salmon", false),
        (9003, "2026-10-26T18:40:00-04:00", "100g glaze", false),
    ];
    for (id, received, text, photo) in typed {
        let m = Msg { spec: Spec { id, received, text, photo }, live: true };
        match pip::handle(&allow, &cfg, &m).await {
            Outcome::Saved => {}
            other => println!("  outcome for {id}: {other:?}"),
        }
    }

    println!("\n== (b) replay: DMs sent while the laptop was off, then the same replay again ==");
    let batch = vec![
        Spec { id: 9101, received: "2026-10-25T20:10:00-04:00", text: "1 tbsp oil", photo: false },
        Spec { id: 9102, received: "2026-10-25T21:30:00-04:00", text: "cabbage, 100g salmon", photo: true },
        Spec { id: 9103, received: "2026-10-26T08:00:00-04:00", text: "oats 40g, banana", photo: false },
    ];
    let history = Replay { msgs: Mutex::new(batch) };
    let handler = PipHandler { allow: &allow, cfg: &cfg };
    catch_up_each(&allow, &nutrition, &history, &handler).await;
    println!("-- second catch-up over the same history --");
    catch_up_each(&allow, &nutrition, &history, &handler).await;

    println!("\n== notes ==");
    show_dir("nutrition folder", &nutrition);
    show_dir("recipes folder (read only)", &recipes);
    for day in ["2026-10-25.md", "2026-10-26.md"] {
        if let Ok(text) = fs::read_to_string(nutrition.join(day)) {
            println!("\n--- {day} ---\n{text}");
        }
    }

    println!("\n== (c) his hand edits survive ==");
    let day = "2026-10-27";
    let day_file = nutrition.join(format!("{day}.md"));
    send(&allow, &cfg, 9201, "2026-10-27T12:15:00-04:00", "100g salmon, 1/2 cup dry rice, cabbage").await;
    let written = fs::read_to_string(&day_file).expect("the first DM wrote the day note");
    let mut edited = format!("---\nmood: tired\nhow full: very\n---\n{written}");
    edited = insert_after(&edited, |l| l == "# 2026-10-27", "Felt bloated after lunch.\n");
    edited = insert_after(&edited, |l| l.starts_with("**Flags:**"), "ate late, ignore the flag\n");
    edited = insert_after(&edited, |l| l == "<!-- /pip:meal 9201 -->", "skipped the cabbage\n");
    fs::write(&day_file, &edited).expect("hand edit");
    println!("--- before Pip writes ---\n{edited}");

    send(&allow, &cfg, 9202, "2026-10-27T18:05:00-04:00", "1 serving salmon bowl, 150g salmon").await;
    let after = fs::read_to_string(&day_file).expect("day note");
    println!("--- after Pip writes ---\n{after}");
    let (k, n) = (kept(&after, &HIS), HIS.len());
    println!("his lines kept: {k}/{n}");

    println!("\n== (c) damaged: the totals close marker deleted by hand ==");
    let cut: String = after.split_inclusive('\n').filter(|l| l.trim_end() != "<!-- /pip:totals -->").collect();
    fs::write(&day_file, &cut).expect("damage the note");
    let warn_before = cut.matches("totals not updated").count();
    send(&allow, &cfg, 9203, "2026-10-27T20:40:00-04:00", "100g glaze").await;
    let damaged = fs::read_to_string(&day_file).expect("day note");
    println!("--- after Pip writes (damaged) ---\n{damaged}");
    let (k, n) = (kept(&damaged, &HIS), HIS.len());
    println!("his lines kept: {k}/{n}");
    let warned = damaged.matches("totals not updated").count() > warn_before;
    println!("warning appended: {}", if warned { "yes" } else { "no" });
}
