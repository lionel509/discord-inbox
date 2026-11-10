//! #146 demo: a label photo saves a food note, then counts log from it, through `pip::handle` on scratch folders.
//! Run with `scripts/demo-146.sh`. Without a reader it uses a stub helper that answers the label photo with made-up
//! "Northwind" values. With `PIP_DEMO_MODEL` and `PIP_DEMO_PHOTO` set, the real reader reads that photo instead.
use discord_inbox::allowlist;
use discord_inbox::ingest::{Inbound, Outcome};
use discord_inbox::pip::{self, Config};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAIN: u64 = 100000000000000001;
const MAIN_CH: u64 = 5001;
const STUB_LABEL: &str = r#"{"label":{"name":"Northwind Pork Wontons","serving_g":140,"serving_pc":8,"kcal":230,"protein":9,"carbs":28,"fat":9},"items":[],"unreadable":[]}"#;

struct Msg {
    id: u64,
    text: String,
    files: Vec<String>,
    photo: Vec<u8>,
    replies: Arc<Mutex<Vec<String>>>,
}

impl Inbound for Msg {
    fn author_id(&self) -> u64 {
        MAIN
    }
    fn msg_id(&self) -> u64 {
        self.id
    }
    fn channel_id(&self) -> u64 {
        MAIN_CH
    }
    fn received(&self) -> String {
        "2026-10-29T12:00:00-04:00".to_string()
    }
    fn content(&self) -> String {
        self.text.clone()
    }
    fn attachment_names(&self) -> Vec<String> {
        self.files.clone()
    }
    fn live(&self) -> bool {
        false
    }
    async fn download(&self, _i: usize) -> Result<Vec<u8>, String> {
        Ok(self.photo.clone())
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
        self.replies.lock().unwrap().push(text.to_string());
        Ok(())
    }
    async fn reply_rich(&self, r: &discord_inbox::rich::Rich) -> Result<(), String> {
        self.replies.lock().unwrap().push(r.text.clone());
        Ok(())
    }
}

/// A `/bin/sh` reader that answers with the label JSON in `stub-label.json` (made-up values).
fn stub(root: &Path) -> PathBuf {
    fs::write(root.join("stub-label.json"), STUB_LABEL).unwrap();
    let p = root.join("stub-helper.sh");
    fs::write(&p, format!("#!/bin/sh\ncat >/dev/null\ncat '{}'\n", root.join("stub-label.json").display())).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

async fn send(c: &Config, id: u64, text: &str, files: &[&str], photo: Vec<u8>) -> (Outcome, String) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let m = Msg { id, text: text.to_string(), files: files.iter().map(|f| f.to_string()).collect(), photo, replies: replies.clone() };
    let allow = allowlist::parse(&format!("{MAIN}\n")).unwrap();
    let out = pip::handle(&allow, c, &m).await;
    let reply = replies.lock().unwrap().join("\n");
    (out, reply)
}

#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("usage: pip_foods_demo <scratch-root>"));
    let _ = fs::remove_dir_all(&root);
    let nutrition = root.join("Private").join("Nutrition");
    let foods = root.join("Private").join("Pantry").join("Foods");
    fs::create_dir_all(&nutrition).unwrap();
    let (model, photo) = match (std::env::var("PIP_DEMO_MODEL"), std::env::var("PIP_DEMO_PHOTO")) {
        (Ok(m), Ok(p)) => (PathBuf::from(m), fs::read(p).expect("PIP_DEMO_PHOTO must be readable")),
        _ => (stub(&root), b"jpeg bytes".to_vec()),
    };
    let c = Config {
        nutrition_dir: nutrition.clone(),
        recipes_dir: None,
        foods_dir: Some(foods.clone()),
        low_kcal: 1800,
        kcal_target: 2400,
        model: Some(model),
        model_timeout: Duration::from_secs(90),
    };
    let steps: [(&str, &[&str]); 4] = [("", &["label.jpg"]), ("24 wontons", &[]), ("wontons", &[]), ("wontons 200g", &[])];
    for (i, (text, files)) in steps.iter().enumerate() {
        let (out, reply) = send(&c, 6001 + i as u64, text, files, photo.clone()).await;
        println!("--- message {} {:?} -> {:?}\n{reply}\n", i + 1, if text.is_empty() { "(label photo)" } else { text }, out);
    }
    println!("=== food note\n{}", fs::read_to_string(foods.join("Northwind Pork Wontons.md")).unwrap_or_default());
    println!("=== index\n{}", fs::read_to_string(foods.join("Foods.md")).unwrap_or_default());
    let day = fs::read_dir(&nutrition).unwrap().filter_map(|e| e.ok().map(|e| e.path())).find(|p| p.extension().is_some_and(|x| x == "md"));
    if let Some(day) = day {
        println!("=== day note\n{}", fs::read_to_string(day).unwrap_or_default());
    }
}
