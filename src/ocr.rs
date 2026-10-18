//! Local OCR of image attachments: Apple Vision through the helper build.rs compiles from src/vision.swift. The text goes only into the note: never a log line, never a file of its own.
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
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
    run_args(cmd, &[], image, timeout)
}

/// `run` with arguments, for any bounded helper that reads its input on stdin (the local model in insight.rs).
pub fn run_args(cmd: &Path, args: &[String], image: &[u8], timeout: Duration) -> Option<String> {
    run_in(cmd, args, image, timeout, None)
}

/// `run_args` in `cwd` when given. The helper leads its own process group, so a timeout kills the grandchildren it started too (#84).
pub fn run_in(cmd: &Path, args: &[String], image: &[u8], timeout: Duration, cwd: Option<&Path>) -> Option<String> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let mut child = command.spawn().ok()?;
    let (Some(mut input), Some(mut output)) = (child.stdin.take(), child.stdout.take()) else {
        kill(&mut child);
        return None;
    };

    // Each pipe gets its own thread, so a helper that writes a lot before reading stdin can't deadlock us.
    let bytes = image.to_vec();
    std::thread::spawn(move || {
        let _ = input.write_all(&bytes);
    });
    // Not joined: a helper that left a grandchild holding stdout must not hang us. The read is waited for only up to the deadline.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = output.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    let deadline = Instant::now() + timeout;
    // Starts short so a quick helper isn't charged a full poll interval; doubles up to POLL.
    let mut nap = Duration::from_millis(1);
    let status: ExitStatus = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(nap);
                nap = (nap * 2).min(POLL);
            }
            _ => {
                kill(&mut child);
                return None;
            }
        }
    };

    if !status.success() {
        return None;
    }
    // The child has exited, but a grandchild may still hold stdout: wait until the deadline, at least 1 s.
    let wait = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_secs(1));
    let out = rx.recv_timeout(wait).ok()?;
    let text = String::from_utf8_lossy(&out).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn kill(child: &mut Child) {
    // Negative pid: the whole process group the helper leads (see run_in), then the helper itself.
    #[cfg(unix)]
    unsafe {
        kill_group(-(child.id() as i32), 9);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn kill_group(pid: i32, sig: i32) -> i32;
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
