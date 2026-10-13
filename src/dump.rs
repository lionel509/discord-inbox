use std::path::{Path, PathBuf};

pub struct Incoming {
    pub msg_id: u64,
    pub author_id: u64,
    pub received: String,
    pub content: String,
    pub attachments: Vec<(String, Vec<u8>)>,
}

pub fn inbox_dir() -> PathBuf {
    todo!()
}

pub fn sha256_hex(_bytes: &[u8]) -> String {
    todo!()
}

pub fn write_note(_inbox: &Path, _msg: &Incoming) -> std::io::Result<PathBuf> {
    todo!()
}

pub fn read_last_id(_inbox: &Path) -> Option<u64> {
    todo!()
}

pub fn write_last_id(_inbox: &Path, _id: u64) -> std::io::Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_inbox() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "di-dump-test-{}-{}",
            std::process::id(),
            n
        ));
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
