use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Incoming {
    pub msg_id: u64,
    pub author_id: u64,
    /// UTC, exactly `YYYY-MM-DDTHH:MM:SSZ`.
    pub received: String,
    pub content: String,
    /// `(original filename, bytes)`.
    pub attachments: Vec<(String, Vec<u8>)>,
}

/// `$DISCORD_INBOX_DIR`, else `~/Documents/Inbox`.
pub fn inbox_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("DISCORD_INBOX_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join("Documents/Inbox")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Keeps ASCII alphanumerics, `.`, `_` and `-`; everything else becomes `_`.
fn sanitize(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() || clean.chars().all(|c| c == '.') {
        "file".to_string()
    } else if clean.len() > MAX_NAME {
        // ASCII only, so byte slicing is safe. Keep the tail so the extension survives.
        clean[clean.len() - MAX_NAME..].to_string()
    } else {
        clean
    }
}

/// Keeps the on-disk name well under the 255-byte filesystem limit.
const MAX_NAME: usize = 100;

/// Writes attachments first, then the note, so a note on disk always has its files.
pub fn write_note(inbox: &Path, msg: &Incoming) -> std::io::Result<PathBuf> {
    fs::create_dir_all(inbox)?;
    let stem = format!(
        "{}_{}_{}",
        &msg.received[..10],
        msg.received[11..19].replace(':', ""),
        msg.msg_id
    );

    let mut entries = Vec::new();
    for (i, (name, bytes)) in msg.attachments.iter().enumerate() {
        let file = format!("{stem}_{}_{}", i + 1, sanitize(name));
        fs::write(inbox.join(&file), bytes)?;
        entries.push(format!(
            "  - file: {file}\n    sha256: {}\n",
            sha256_hex(bytes)
        ));
    }

    let attachments = if entries.is_empty() {
        "attachments: []\n".to_string()
    } else {
        format!("attachments:\n{}", entries.concat())
    };
    let text = format!(
        "---\nreceived: {}\nfrom: {}\nmsg_id: {}\nlane: dump\n{attachments}---\n{}\n",
        msg.received, msg.author_id, msg.msg_id, msg.content
    );

    let note = inbox.join(format!("{stem}.md"));
    fs::write(&note, text)?;
    Ok(note)
}

const LAST_ID: &str = ".last_msg_id";

pub fn read_last_id(inbox: &Path) -> Option<u64> {
    fs::read_to_string(inbox.join(LAST_ID))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Only ever moves forward. The lock stops two writers (live event and catch-up)
/// from sharing one tmp file or interleaving check-then-write.
pub fn write_last_id(inbox: &Path, id: u64) -> std::io::Result<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if read_last_id(inbox).is_some_and(|cur| cur >= id) {
        return Ok(());
    }
    let tmp = inbox.join(format!("{LAST_ID}.tmp"));
    fs::write(&tmp, format!("{id}\n"))?;
    fs::rename(tmp, inbox.join(LAST_ID))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_inbox() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("di-dump-test-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(attachments: Vec<(&str, &[u8])>) -> Incoming {
        Incoming {
            msg_id: 1427000000000000000,
            author_id: 100000000000000001,
            received: "2026-10-13T14:03:22Z".to_string(),
            content: "hi".to_string(),
            attachments: attachments
                .into_iter()
                .map(|(n, b)| (n.to_string(), b.to_vec()))
                .collect(),
        }
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn note_filename_and_frontmatter() {
        let inbox = temp_inbox();
        let stem = "2026-10-13_140322_1427000000000000000";
        let note = write_note(&inbox, &sample(vec![("photo.jpg", b"xyz")])).unwrap();
        assert_eq!(note, inbox.join(format!("{stem}.md")));
        let att = inbox.join(format!("{stem}_1_photo.jpg"));
        assert_eq!(fs::read(&att).unwrap(), b"xyz");
        let expected = format!(
            "---\nreceived: 2026-10-13T14:03:22Z\nfrom: 100000000000000001\nmsg_id: 1427000000000000000\nlane: dump\nattachments:\n  - file: {stem}_1_photo.jpg\n    sha256: {}\n---\nhi\n",
            sha256_hex(b"xyz")
        );
        assert_eq!(fs::read_to_string(&note).unwrap(), expected);
    }

    #[test]
    fn no_attachments_writes_empty_list() {
        let inbox = temp_inbox();
        let note = write_note(&inbox, &sample(vec![])).unwrap();
        let text = fs::read_to_string(note).unwrap();
        assert!(text.contains("attachments: []\n"), "{text}");
        assert!(!text.contains("- file:"), "{text}");
    }

    #[test]
    fn attachment_name_is_sanitized() {
        let inbox = temp_inbox();
        write_note(&inbox, &sample(vec![("../../evil sh.txt", b"x")])).unwrap();
        let names: Vec<String> = fs::read_dir(&inbox)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".txt"))
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].ends_with("_1_.._.._evil_sh.txt"), "{names:?}");
        assert!(!names[0].contains('/') && !names[0].contains(' '));
    }

    #[test]
    fn long_attachment_name_is_dumped() {
        let inbox = temp_inbox();
        let long = format!(
            "{}.png",
            "screenshot-of-a-very-long-window-title-".repeat(6)
        );
        assert!(long.len() > 200 && long.len() < 255);
        write_note(&inbox, &sample(vec![(long.as_str(), b"png")])).unwrap();
        let files: Vec<String> = fs::read_dir(&inbox)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".png"))
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        assert!(files[0].len() <= 255, "{}", files[0].len());
    }

    #[test]
    fn concurrent_cursor_writes_keep_max() {
        let inbox = temp_inbox();
        let mut handles = Vec::new();
        for t in 0..8u64 {
            let inbox = inbox.clone();
            handles.push(std::thread::spawn(move || {
                let mut errs = Vec::new();
                for i in 0..200u64 {
                    let id = 1000 + t * 200 + i;
                    if let Err(e) = write_last_id(&inbox, id) {
                        errs.push(format!("id {id}: {e}"));
                    }
                }
                errs
            }));
        }
        let mut errs: Vec<String> = Vec::new();
        for h in handles {
            errs.extend(h.join().unwrap());
        }
        assert!(
            errs.is_empty(),
            "{} errors, first: {:?}",
            errs.len(),
            errs.first()
        );
        assert_eq!(read_last_id(&inbox), Some(1000 + 7 * 200 + 199));
    }

    #[test]
    fn last_id_only_advances() {
        let inbox = temp_inbox();
        assert_eq!(read_last_id(&inbox), None);
        write_last_id(&inbox, 10).unwrap();
        assert_eq!(read_last_id(&inbox), Some(10));
        write_last_id(&inbox, 5).unwrap();
        assert_eq!(read_last_id(&inbox), Some(10));
        write_last_id(&inbox, 11).unwrap();
        assert_eq!(read_last_id(&inbox), Some(11));
        assert!(!inbox.join(".last_msg_id.tmp").exists());
    }
}
