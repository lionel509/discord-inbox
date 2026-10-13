//! Local OCR of image attachments: Apple Vision through the helper build.rs compiles from src/vision.swift. The text goes only into the note: never a log line, never a file of its own.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Ceiling for one image. The first call after a build or boot took ~45 s here; warm calls take ~0.2 s.
pub const TIMEOUT: Duration = Duration::from_secs(120);

const POLL: Duration = Duration::from_millis(50);
const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "heic", "heif", "webp", "gif", "tif", "tiff", "bmp",
];

/// `$DISCORD_INBOX_OCR` if set, else the helper build.rs compiled. Empty when there is none, which spawns nothing.
pub fn command() -> PathBuf {
    match std::env::var_os("DISCORD_INBOX_OCR") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(option_env!("DISCORD_INBOX_VISION").unwrap_or("")),
    }
}

// ponytail: by extension only; an image sent with no extension is not read. Sniff magic bytes if that ever shows up.
pub fn is_image(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Runs the helper on `image` (stdin) and returns its stdout, trimmed. `None` when it can't spawn,
/// exits non-zero, times out, or prints nothing. Its stderr is discarded, so nothing it says reaches a log.
pub fn run(cmd: &Path, image: &[u8], timeout: Duration) -> Option<String> {
    let mut child = Command::new(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let (Some(mut input), Some(mut output)) = (child.stdin.take(), child.stdout.take()) else {
        kill(&mut child);
        return None;
    };

    // Each pipe gets its own thread, so a helper that writes a lot before reading stdin can't deadlock us.
    let bytes = image.to_vec();
    std::thread::spawn(move || {
        let _ = input.write_all(&bytes);
    });
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = output.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let status: ExitStatus = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            _ => {
                kill(&mut child);
                // The reader is not joined: a helper that left a grandchild on stdout must not hang us here.
                return None;
            }
        }
    };

    let out = reader.join().ok()?;
    if !status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// OCR text for each image attachment, as `(attachment index, text)`. Runs the helper on a blocking thread
/// per image. A failure logs the attachment's number and nothing about its contents.
pub async fn read_all(
    msg_id: u64,
    attachments: &[(String, Option<Vec<u8>>)],
) -> Vec<(usize, String)> {
    let cmd = command();
    let mut out = Vec::new();
    for (i, (name, bytes)) in attachments.iter().enumerate() {
        let Some(bytes) = bytes else { continue };
        if !is_image(name) {
            continue;
        }
        let (cmd, bytes) = (cmd.clone(), bytes.clone());
        match tokio::task::spawn_blocking(move || run(&cmd, &bytes, TIMEOUT)).await {
            Ok(Some(text)) => out.push((i, text)),
            _ => eprintln!(
                "discord-inbox: no ocr text for attachment {} of {msg_id}",
                i + 1
            ),
        }
    }
    out
}
