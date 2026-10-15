use crate::allowlist::Allowlist;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Incoming {
    pub msg_id: u64,
    pub author_id: u64,
    /// UTC, exactly `YYYY-MM-DDTHH:MM:SSZ`.
    pub received: String,
    pub content: String,
    /// `(original filename, bytes)`. `None` means the download failed: the note lists the
    /// attachment as `sha256: pending` and writes no file for it.
    pub attachments: Vec<(String, Option<Vec<u8>>)>,
    /// (attachment index, text) from ocr::read_all, written under "## OCR <file>" after the content.
    pub ocr: Vec<(usize, String)>,
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

fn stem(received: &str, msg_id: u64) -> String {
    format!(
        "{}_{}_{}",
        &received[..10],
        received[11..19].replace(':', ""),
        msg_id
    )
}

/// Where the note for a message lives. The skip check and `write_note` both use this.
pub fn note_path(inbox: &Path, received: &str, msg_id: u64) -> PathBuf {
    inbox.join(format!("{}.md", stem(received, msg_id)))
}

/// The note text for a message, plus the attachment file names it lists, in order.
/// `front_extra` goes after `lane: dump` in the frontmatter, `lead` right after the closing `---`.
/// Both empty gives the plain note `write_note` has always written.
fn note_text(msg: &Incoming, front_extra: &str, lead: &str) -> (String, Vec<String>) {
    let base = stem(&msg.received, msg.msg_id);

    let mut entries = Vec::new();
    let mut files = Vec::new();
    for (i, (name, bytes)) in msg.attachments.iter().enumerate() {
        let file = format!("{base}_{}_{}", i + 1, sanitize(name));
        match bytes {
            Some(b) => entries.push(format!("  - file: {file}\n    sha256: {}\n", sha256_hex(b))),
            None => entries.push(format!("  - file: {file}\n    sha256: pending\n")),
        }
        files.push(file);
    }

    let attachments = if entries.is_empty() {
        "attachments: []\n".to_string()
    } else {
        format!("attachments:\n{}", entries.concat())
    };
    // Every note on disk, Inbox or filed, is redacted here: the body and the OCR text.
    let content = crate::redact::sensitive(&msg.content);
    let mut text = format!(
        "---\nreceived: {}\nfrom: {}\nmsg_id: {}\nlane: dump\n{front_extra}{attachments}---\n{lead}{content}\n",
        msg.received, msg.author_id, msg.msg_id
    );
    for (i, raw) in &msg.ocr {
        if let Some(f) = files.get(*i) {
            let t = crate::redact::sensitive(raw);
            // Longer than any tilde run in the text, so no line of it can close the fence early.
            let longest = t.split(|c: char| c != '~').map(str::len).max().unwrap_or(0);
            let fence = "~".repeat(3.max(longest + 1));
            text.push_str(&format!("\n## OCR {f}\n\n{fence}text\n{t}\n{fence}\n"));
        }
    }
    (text, files)
}

/// Writes attachments first, then the note, so a note on disk always has its files.
pub fn write_note(inbox: &Path, msg: &Incoming) -> std::io::Result<PathBuf> {
    fs::create_dir_all(inbox)?;
    let (text, files) = note_text(msg, "", "");
    for (file, (_, bytes)) in files.iter().zip(&msg.attachments) {
        if let Some(b) = bytes {
            fs::write(inbox.join(file), b)?;
        }
    }

    let note = note_path(inbox, &msg.received, msg.msg_id);
    // Written to a temp file, then renamed, so a note at its final path is always whole.
    let tmp = note.with_extension("md.tmp");
    fs::write(&tmp, text)?;
    if let Err(e) = fs::rename(&tmp, &note) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(note)
}

/// A non-empty note whose frontmatter lists no `sha256: pending` attachment. Only the frontmatter counts:
/// the body holds DM and OCR text, which can say anything.
pub fn note_is_complete(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|t| {
        !t.is_empty() && !t.split("\n---\n").next().unwrap_or("").contains("sha256: pending")
    })
}

/// A non-empty note is on disk. A zero-length one is left by a crash, so it does not count as saved.
pub fn note_saved(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// Serialises every writer of `.cursor-*`, `.retry` and notes. The live event and catch-up can run at once.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The lib gate. A sender not on the allowlist gets no file. Never touches cursors.
/// A pending note never replaces a complete one, so the check and the write share the lock.
pub fn accept(allow: &Allowlist, inbox: &Path, msg: &Incoming) -> std::io::Result<bool> {
    if !allow.contains(msg.author_id) {
        return Ok(false);
    }
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let pending = msg.attachments.iter().any(|(_, b)| b.is_none());
    if pending && note_is_complete(&note_path(inbox, &msg.received, msg.msg_id)) {
        // Keep the complete note. The retry entry stays until the next start clears it.
        return Ok(true);
    }
    write_note(inbox, msg)?;
    Ok(true)
}

/// The saved cursor for one DM channel, or `None` when there is none (or it is 0 or junk).
pub fn read_cursor(inbox: &Path, channel: u64) -> Option<u64> {
    fs::read_to_string(inbox.join(format!(".cursor-{channel}")))
        .ok()?
        .trim()
        .parse()
        .ok()
        .filter(|&id: &u64| id != 0)
}

/// Only ever moves forward. The lock stops two writers from sharing one tmp file or
/// interleaving check-then-write.
pub fn write_cursor(inbox: &Path, channel: u64, id: u64) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if read_cursor(inbox, channel).is_some_and(|c| c >= id) {
        return Ok(());
    }
    fs::create_dir_all(inbox)?;
    let tmp = inbox.join(format!(".cursor-{channel}.tmp"));
    fs::write(&tmp, format!("{id}\n"))?;
    fs::rename(tmp, inbox.join(format!(".cursor-{channel}")))
}

const RETRY: &str = ".retry";

/// `(channel, msg id)` pairs still owed a complete note. Junk and zero IDs are skipped.
pub fn retry_entries(inbox: &Path) -> Vec<(u64, u64)> {
    let Ok(text) = fs::read_to_string(inbox.join(RETRY)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 2 {
                return None;
            }
            let channel: u64 = parts[0].parse().ok()?;
            let id: u64 = parts[1].parse().ok()?;
            (channel != 0 && id != 0).then_some((channel, id))
        })
        .collect()
}

pub fn in_retry(inbox: &Path, msg_id: u64) -> bool {
    retry_entries(inbox).iter().any(|e| e.1 == msg_id)
}

fn write_retry(inbox: &Path, entries: &[(u64, u64)]) -> std::io::Result<()> {
    let text: String = entries.iter().map(|(c, m)| format!("{c} {m}\n")).collect();
    let tmp = inbox.join(format!("{RETRY}.tmp"));
    fs::write(&tmp, text)?;
    fs::rename(tmp, inbox.join(RETRY))
}

/// Records a message that still owes an attachment. Adding it twice is a no-op.
pub fn add_retry(inbox: &Path, channel: u64, msg_id: u64) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut entries = retry_entries(inbox);
    if entries.iter().any(|e| e.1 == msg_id) {
        return Ok(());
    }
    entries.push((channel, msg_id));
    fs::create_dir_all(inbox)?;
    write_retry(inbox, &entries)
}

/// Clears a message from the retry list. Without a matching entry it touches nothing on disk.
pub fn remove_retry(inbox: &Path, msg_id: u64) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let entries = retry_entries(inbox);
    if !entries.iter().any(|e| e.1 == msg_id) {
        return Ok(());
    }
    let left: Vec<(u64, u64)> = entries.into_iter().filter(|e| e.1 != msg_id).collect();
    if left.is_empty() {
        return fs::remove_file(inbox.join(RETRY));
    }
    write_retry(inbox, &left)
}

const FILED: &str = ".filed";

/// The folder a message was filed into, from the `.filed` ledger. Junk lines are skipped.
pub fn filed_to(inbox: &Path, msg_id: u64) -> Option<String> {
    let text = fs::read_to_string(inbox.join(FILED)).ok()?;
    text.lines().rev().find_map(|line| {
        let (id, dest) = line.split_once(' ')?;
        (id.parse::<u64>().ok()? == msg_id).then(|| dest.to_string())
    })
}

/// Removes an Inbox copy that is already filed. A missing file is fine: duplicates are harmless.
fn drop_inbox_copy(p: &Path) {
    if let Err(e) = fs::remove_file(p) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("discord-inbox: could not remove {}: {e}", p.display());
        }
    }
}

/// Writes `text` to a brand-new file at `path`. A stale temp file goes first; a planted symlink is removed
/// as a link, never followed. `create_new` is O_EXCL, so nothing is ever written through an existing path.
fn write_new(path: &Path, text: &str) -> std::io::Result<()> {
    let _ = fs::remove_file(path);
    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(text.as_bytes())
}

/// Moves a message's note and attachments from Inbox into `dest_dir/name` and records it in the
/// `.filed` ledger. Hard links, so an existing file is never overwritten and a clash leaves Inbox
/// untouched. Same volume only: a cross-volume link fails and is rolled back.
pub fn file_away(
    inbox: &Path,
    msg: &Incoming,
    dest_dir: &Path,
    name: &str,
    front_extra: &str,
    lead: &str,
) -> std::io::Result<PathBuf> {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if !dest_dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no folder {}", dest_dir.display()),
        ));
    }
    let (text, files) = note_text(msg, front_extra, lead);
    let dest = dest_dir.join(name);
    let tmp = dest_dir.join(format!(".{name}.tmp"));
    let ledger = inbox.join(FILED);

    // Every link this call creates goes here, so a failure removes exactly what it added.
    let mut made: Vec<PathBuf> = Vec::new();
    let linked = (|| -> std::io::Result<()> {
        for file in &files {
            let src = inbox.join(file);
            if !src.is_file() {
                continue; // a pending attachment has no file yet
            }
            let to = dest_dir.join(file);
            fs::hard_link(&src, &to)?;
            made.push(to);
        }
        write_new(&tmp, &text)?;
        fs::hard_link(&tmp, &dest)?;
        made.push(dest.clone());
        fs::remove_file(&tmp)?;
        let mut ledger_text = fs::read_to_string(&ledger).unwrap_or_default();
        ledger_text.push_str(&format!("{} {}\n", msg.msg_id, dest.display()));
        let ledger_tmp = inbox.join(format!("{FILED}.tmp"));
        write_new(&ledger_tmp, &ledger_text)?;
        fs::rename(&ledger_tmp, &ledger)
    })();
    if let Err(e) = linked {
        for p in &made {
            let _ = fs::remove_file(p);
        }
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    for file in &files {
        drop_inbox_copy(&inbox.join(file));
    }
    drop_inbox_copy(&note_path(inbox, &msg.received, msg.msg_id));
    Ok(dest)
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
            ocr: Vec::new(),
            attachments: attachments
                .into_iter()
                .map(|(n, b)| (n.to_string(), Some(b.to_vec())))
                .collect(),
        }
    }

    #[test]
    fn accept_ignores_strangers() {
        let allow = crate::allowlist::parse("100000000000000001\n100000000000000002\n").unwrap();
        let inbox = temp_inbox().join("Inbox");
        let mut stranger = sample(vec![("a.txt", b"x")]);
        stranger.author_id = 999999999999999999;
        assert!(!accept(&allow, &inbox, &stranger).unwrap());
        assert!(!inbox.exists(), "no dir, note or cursor for a stranger");
        let mut alt = sample(vec![]);
        alt.author_id = 100000000000000002;
        assert!(
            accept(&allow, &inbox, &alt).unwrap(),
            "the alt account is admitted"
        );
    }

    #[test]
    fn accept_writes_for_allowed() {
        let allow = crate::allowlist::parse("100000000000000001\n100000000000000002\n").unwrap();
        let inbox = temp_inbox().join("Inbox");
        let mut alt = sample(vec![]);
        alt.author_id = 100000000000000002;
        assert!(accept(&allow, &inbox, &alt).unwrap());
        assert!(inbox
            .join("2026-10-13_140322_1427000000000000000.md")
            .exists());
        assert_eq!(
            read_cursor(&inbox, 7001),
            None,
            "accept never writes a cursor"
        );
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
    fn pending_attachment_is_listed_without_a_file() {
        let inbox = temp_inbox();
        let msg = Incoming {
            msg_id: 1427000000000000000,
            author_id: 100000000000000001,
            received: "2026-10-13T14:03:22Z".to_string(),
            content: "hi".to_string(),
            ocr: Vec::new(),
            attachments: vec![
                ("a.txt".to_string(), Some(b"x".to_vec())),
                ("b.jpg".to_string(), None),
            ],
        };
        let note = write_note(&inbox, &msg).unwrap();
        let stem = "2026-10-13_140322_1427000000000000000";
        let expected = format!(
            "---\nreceived: 2026-10-13T14:03:22Z\nfrom: 100000000000000001\nmsg_id: 1427000000000000000\nlane: dump\nattachments:\n  - file: {stem}_1_a.txt\n    sha256: {}\n  - file: {stem}_2_b.jpg\n    sha256: pending\n---\nhi\n",
            sha256_hex(b"x")
        );
        assert_eq!(fs::read_to_string(&note).unwrap(), expected);
        assert!(!inbox.join(format!("{stem}_2_b.jpg")).exists());
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
                    if let Err(e) = write_cursor(&inbox, 7001, id) {
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
        assert_eq!(read_cursor(&inbox, 7001), Some(1000 + 7 * 200 + 199));
    }

    #[test]
    fn cursor_only_advances() {
        let inbox = temp_inbox();
        assert_eq!(read_cursor(&inbox, 7001), None);
        write_cursor(&inbox, 7001, 10).unwrap();
        assert_eq!(read_cursor(&inbox, 7001), Some(10));
        write_cursor(&inbox, 7001, 5).unwrap();
        assert_eq!(read_cursor(&inbox, 7001), Some(10));
        write_cursor(&inbox, 7001, 11).unwrap();
        assert_eq!(read_cursor(&inbox, 7001), Some(11));
        assert!(!inbox.join(".cursor-7001.tmp").exists());
    }

    #[test]
    fn cursors_are_per_channel() {
        let inbox = temp_inbox();
        write_cursor(&inbox, 7001, 50).unwrap();
        write_cursor(&inbox, 7002, 20).unwrap();
        assert_eq!(read_cursor(&inbox, 7001), Some(50));
        assert_eq!(read_cursor(&inbox, 7002), Some(20));
    }

    #[test]
    fn read_cursor_ignores_zero_and_junk() {
        let inbox = temp_inbox();
        fs::write(inbox.join(".cursor-7001"), "0\n").unwrap();
        fs::write(inbox.join(".cursor-7002"), "abc\n").unwrap();
        assert_eq!(read_cursor(&inbox, 7001), None);
        assert_eq!(read_cursor(&inbox, 7002), None);
    }

    #[test]
    fn retry_add_is_idempotent_and_remove_clears() {
        let inbox = temp_inbox();
        add_retry(&inbox, 7001, 5).unwrap();
        add_retry(&inbox, 7001, 5).unwrap();
        add_retry(&inbox, 7002, 6).unwrap();
        assert_eq!(retry_entries(&inbox), vec![(7001, 5), (7002, 6)]);
        remove_retry(&inbox, 5).unwrap();
        assert_eq!(retry_entries(&inbox), vec![(7002, 6)]);
        remove_retry(&inbox, 6).unwrap();
        assert!(!inbox.join(".retry").exists());
    }

    #[test]
    fn retry_entries_skip_junk_and_zero() {
        let inbox = temp_inbox();
        fs::write(
            inbox.join(".retry"),
            "7001 5\njunk\n0 6\n7002 0\n7003 7 8\n",
        )
        .unwrap();
        assert_eq!(retry_entries(&inbox), vec![(7001, 5)]);
    }

    #[test]
    fn remove_retry_of_absent_id_touches_nothing() {
        let inbox = temp_inbox().join("Inbox");
        remove_retry(&inbox, 5).unwrap();
        assert!(
            !inbox.exists(),
            "a live message with no retry must not create files"
        );
    }
}
