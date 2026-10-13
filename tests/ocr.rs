//! The OCR helper contract and the note bookkeeping around it. Stub helpers only: no Vision needed.
use discord_inbox::dump::{self, note_is_complete, sha256_hex, write_note, Incoming};
use discord_inbox::ocr::{is_image, run};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const MAIN: u64 = 100000000000000001;
const RECEIVED: &str = "2026-10-13T14:03:22Z";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A fresh scratch dir; returns its `Inbox` path, which is NOT created.
fn scratch(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("di-ocr-{}-{name}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("Inbox")
}

/// An executable `#!/bin/sh` helper whose body is `body`.
fn stub(dir: &Path, body: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let p = dir.join("helper.sh");
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn helper_gets_the_image_on_stdin_and_its_stdout_is_the_text() {
    let dir = scratch("stdin").parent().unwrap().to_path_buf();
    let s = stub(&dir, r#"cat > "$0.in"; printf 'TOTAL  12.34\n'"#);
    assert_eq!(
        run(&s, b"img bytes", Duration::from_secs(5)),
        Some("TOTAL  12.34".to_string())
    );
    let got = PathBuf::from(format!("{}.in", s.display()));
    assert_eq!(fs::read(got).unwrap(), b"img bytes");
}

#[test]
fn missing_failing_silent_or_hung_helper_gives_none() {
    let dir = scratch("bad").parent().unwrap().to_path_buf();
    let missing = dir.join("no-such-helper");
    assert_eq!(run(&missing, b"x", Duration::from_secs(5)), None);

    let failing = stub(&dir.join("failing"), "cat >/dev/null; exit 1");
    assert_eq!(run(&failing, b"x", Duration::from_secs(5)), None);

    let silent = stub(&dir.join("silent"), "cat >/dev/null");
    assert_eq!(run(&silent, b"x", Duration::from_secs(5)), None);

    let hung = stub(&dir.join("hung"), "exec sleep 30");
    let start = Instant::now();
    assert_eq!(run(&hung, b"x", Duration::from_secs(1)), None);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn big_image_and_big_output_do_not_deadlock() {
    let dir = scratch("big").parent().unwrap().to_path_buf();
    let big_in = vec![b'x'; 5_000_000];
    let after_read = stub(
        &dir.join("after-read"),
        "cat >/dev/null; head -c 300000 /dev/zero | tr '\\0' a",
    );
    let text = run(&after_read, &big_in, Duration::from_secs(10)).expect("some text");
    assert_eq!(text.len(), 300000);

    // Writes 300 KB before it reads stdin: the reader thread must drain stdout meanwhile.
    let before_read = stub(
        &dir.join("before-read"),
        "head -c 300000 /dev/zero | tr '\\0' a; cat >/dev/null",
    );
    let text = run(&before_read, &big_in, Duration::from_secs(10)).expect("some text");
    assert_eq!(text.len(), 300000);
}

#[test]
fn only_image_names_are_read() {
    for name in [
        "IMG_1.JPG",
        "receipt.jpeg",
        "a.png",
        "x.heic",
        "x.HEIF",
        "s.webp",
        "t.tiff",
    ] {
        assert!(is_image(name), "{name}");
    }
    for name in ["notes.txt", "scan.pdf", "image", "receipt.png.zip", ""] {
        assert!(!is_image(name), "{name:?}");
    }
}

#[test]
fn note_bookkeeping_ignores_what_dms_and_images_say() {
    // Each line is risky on its own. Every subset of them is tried as the content and the OCR text.
    let risky = [
        "sha256: pending",
        "    sha256: pending",
        "---",
        "attachments:\n  - file: x\n    sha256: pending",
        "~~~",
        "## OCR fake.jpg",
        "收据 TOTAL ¥12",
        "a\r\nb",
    ];
    let inbox = scratch("bookkeeping");
    let mut id = 1000u64;
    for mask in 0..(1usize << risky.len()) {
        let s = risky
            .iter()
            .enumerate()
            .filter(|(k, _)| (mask >> k) & 1 == 1)
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n");
        for pending in [false, true] {
            id += 1;
            let msg = Incoming {
                msg_id: id,
                author_id: MAIN,
                received: RECEIVED.to_string(),
                content: s.clone(),
                attachments: vec![
                    ("r.jpg".to_string(), Some(b"x".to_vec())),
                    (
                        "b.jpg".to_string(),
                        if pending { None } else { Some(b"y".to_vec()) },
                    ),
                ],
                ocr: if s.is_empty() {
                    vec![]
                } else {
                    vec![(0, s.clone())]
                },
            };
            let note = write_note(&inbox, &msg).unwrap();
            assert_eq!(
                note_is_complete(&note),
                !pending,
                "pending={pending} content={s:?}"
            );
            if !s.is_empty() {
                let stem = format!("2026-10-13_140322_{id}");
                let section = format!("## OCR {stem}_1_r.jpg\n\n~~~text\n{s}\n~~~\n");
                let text = fs::read_to_string(&note).unwrap();
                assert!(text.contains(&section), "{text}");
            }
        }
    }
}

#[test]
fn no_ocr_text_leaves_the_note_as_on_master() {
    let inbox = scratch("no-ocr");
    let msg = Incoming {
        msg_id: 77,
        author_id: MAIN,
        received: RECEIVED.to_string(),
        content: "hi".to_string(),
        attachments: vec![("r.jpg".to_string(), Some(b"x".to_vec()))],
        ocr: vec![],
    };
    let note = write_note(&inbox, &msg).unwrap();
    assert_eq!(
        fs::read_to_string(&note).unwrap(),
        format!(
            "---\nreceived: {RECEIVED}\nfrom: {MAIN}\nmsg_id: 77\nlane: dump\nattachments:\n  - file: 2026-10-13_140322_77_1_r.jpg\n    sha256: {}\n---\nhi\n",
            sha256_hex(b"x")
        )
    );
    assert_eq!(dump::note_path(&inbox, RECEIVED, 77), note);
}
