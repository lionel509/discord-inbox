//! The OCR helper contract and the note bookkeeping around it. Stub helpers only: no Vision needed.
use discord_inbox::dump::{self, note_is_complete, sha256_hex, write_note, Incoming};
use discord_inbox::ocr::{is_image, read_all_with, run};
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

/// The fence the note must use for `t`: longer than any run of `~` in it, and at least three.
fn fence_for(t: &str) -> String {
    let longest = t.split(|c: char| c != '~').map(str::len).max().unwrap_or(0);
    "~".repeat(3.max(longest + 1))
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
                let f = fence_for(&s);
                let section = format!("## OCR {stem}_1_r.jpg\n\n{f}text\n{s}\n{f}\n");
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

#[test]
fn ocr_fence_outlasts_tildes_in_text() {
    let texts = [
        "~~~".to_string(),
        "TOTAL 12.34\n~~~~~~~~~~\nVISA 1234".to_string(),
        "x\n~~~\n\n## OCR fake_1_fake.jpg\n\n~~~text\nFAKE\n~~~".to_string(),
    ];
    let inbox = scratch("fence");
    for (k, t) in texts.iter().enumerate() {
        let msg = Incoming {
            msg_id: 500 + k as u64,
            author_id: MAIN,
            received: RECEIVED.to_string(),
            content: "hi".to_string(),
            attachments: vec![("r.jpg".to_string(), Some(b"x".to_vec()))],
            ocr: vec![(0, t.clone())],
        };
        let note = write_note(&inbox, &msg).unwrap();
        let text = fs::read_to_string(&note).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let fence = fence_for(t);
        let open = lines
            .iter()
            .position(|l| l.starts_with("## OCR "))
            .expect("an OCR section");
        assert_eq!(lines[open + 1], "", "{text}");
        let body = open + 2;
        assert_eq!(lines[body], format!("{fence}text"), "{text}");
        // CommonMark: the closer is the first later line of only tildes, at least as long as the opener.
        let close = (body + 1..lines.len())
            .find(|&i| lines[i].len() >= fence.len() && lines[i].bytes().all(|b| b == b'~'))
            .expect("a closing fence");
        assert_eq!(lines[body + 1..close].join("\n"), *t, "{text}");
        assert_eq!(lines[close], fence, "{text}");
        // Nothing outside the fenced block looks like an OCR section.
        let mut outside = lines[..open].iter().chain(&lines[close + 1..]);
        assert!(outside.all(|l| !l.starts_with("## OCR ")), "{text}");
    }
}

#[test]
fn helper_child_holding_stdout_after_exit_is_bounded() {
    let dir = scratch("grandchild").parent().unwrap().to_path_buf();
    // The helper exits 0 at once, but a background child keeps its stdout open.
    let s = stub(&dir, r#"(sleep 30) & printf 'TOTAL  12.34\n'"#);
    let start = Instant::now();
    assert_eq!(run(&s, b"x", Duration::from_secs(2)), None);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn helper_that_exits_non_zero_with_output_gives_none() {
    let dir = scratch("failing-output").parent().unwrap().to_path_buf();
    let failing = stub(&dir, "cat >/dev/null; printf 'partial\\n'; exit 1");
    assert_eq!(run(&failing, b"x", Duration::from_secs(5)), None);
}

#[test]
fn quick_helpers_are_not_paced_by_the_poll_interval() {
    let dir = scratch("quick").parent().unwrap().to_path_buf();
    let s = stub(&dir, "cat >/dev/null; printf 'x\\n'");
    let start = Instant::now();
    for _ in 0..10 {
        assert_eq!(run(&s, b"x", Duration::from_secs(5)), Some("x".to_string()));
    }
    assert!(start.elapsed() < Duration::from_millis(300), "{:?}", start.elapsed());
}

#[tokio::test]
async fn read_all_with_runs_the_helper_it_is_given_not_the_env_one() {
    let dir = scratch("with").parent().unwrap().to_path_buf();
    let wrong = stub(&dir.join("wrong"), "cat >/dev/null; printf 'WRONG\\n'");
    let right = stub(&dir.join("right"), "cat >/dev/null; printf 'RIGHT\\n'");
    std::env::set_var("DISCORD_INBOX_OCR", &wrong);
    let got = read_all_with(&right, 7, &[("r.jpg".to_string(), Some(b"img".to_vec()))]).await;
    std::env::remove_var("DISCORD_INBOX_OCR");
    assert_eq!(got, vec![(0, "RIGHT".to_string())]);
}
