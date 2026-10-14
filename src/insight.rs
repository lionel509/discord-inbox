//! The reply under a receipt DM: the OCR text with card digits masked, a one-line verdict from a local model, and the budget left for the month. The text goes only into the reply: never a log line, never a file. This module writes nothing; the budget note is read-only.
use crate::dump::Incoming;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One model call. A cold load of a 4B model is about 5-10 s here.
pub const TIMEOUT: Duration = Duration::from_secs(60);
/// Discord's cap on one message, counted in chars.
pub const DISCORD_MAX: usize = 2000;
const PROMPT_CHARS: usize = 4000;
const DEFAULT_CMD: &[&str] = &["ollama", "run", "qwen3.5:4b", "--think=false", "--nowordwrap"];

/// `$DISCORD_INBOX_INSIGHT` split on whitespace if set (empty disables the model), else the default `ollama run`.
pub fn command() -> Vec<String> {
    match std::env::var("DISCORD_INBOX_INSIGHT") {
        Ok(v) => v.split_ascii_whitespace().map(str::to_string).collect(),
        Err(_) => DEFAULT_CMD.iter().map(|s| s.to_string()).collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insight {
    pub receipt: bool,
    pub merchant: String,
    pub total: String,
    pub category: String,
}

/// The prompt sent on stdin: the instructions, a blank line, then the OCR text (first 4000 chars).
pub fn prompt(ocr: &str) -> String {
    let text: String = ocr.chars().take(PROMPT_CHARS).collect();
    format!(
        "You classify text read from a photo. Reply with exactly these four lines and nothing else:\n\
         receipt: yes or no\n\
         merchant: <name or unknown>\n\
         total: <number or unknown>\n\
         category: one of groceries, dining, games, household, transport, school, other\n\
         \n\
         Text:\n{text}"
    )
}

/// Reads `key: value` lines. `receipt` is required; the rest default to "unknown".
pub fn parse(output: &str) -> Option<Insight> {
    let mut receipt = None;
    let (mut merchant, mut total, mut category) = (String::new(), String::new(), String::new());
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "receipt" => receipt = Some(matches!(value.chars().next(), Some('y' | 'Y'))),
            "merchant" => merchant = value.to_string(),
            "total" => total = value.to_string(),
            "category" => category = value.to_ascii_lowercase(),
            _ => {}
        }
    }
    Some(Insight {
        receipt: receipt?,
        merchant: or_unknown(merchant),
        total: or_unknown(total),
        category: or_unknown(category),
    })
}

fn or_unknown(v: String) -> String {
    if v.is_empty() {
        "unknown".to_string()
    } else {
        v
    }
}

/// Runs `cmd` with the prompt on stdin. An empty `cmd` spawns nothing and returns `None`.
pub fn ask(cmd: &[String], ocr: &str, timeout: Duration) -> Option<Insight> {
    let (program, args) = cmd.split_first()?;
    let out = crate::ocr::run_args(Path::new(program), args, prompt(ocr).as_bytes(), timeout)?;
    parse(&out)
}

/// Masks each run of 12 or more digits (single spaces or dashes allowed between digits) except its last four.
pub fn mask_cards(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        loop {
            if j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            } else if j + 1 < chars.len()
                && (chars[j] == ' ' || chars[j] == '-')
                && chars[j + 1].is_ascii_digit()
            {
                j += 2;
            } else {
                break;
            }
        }
        let span = &chars[i..j];
        let digits = span.iter().filter(|c| c.is_ascii_digit()).count();
        if digits >= 12 {
            let mut seen = 0;
            for &c in span {
                if c.is_ascii_digit() {
                    seen += 1;
                    out.push(if seen + 4 <= digits { '*' } else { c });
                } else {
                    out.push(c);
                }
            }
        } else {
            out.extend(span);
        }
        i = j;
    }
    out
}

/// `$` then digits, with optional `,` thousands and `.dd`. Returns cents and the byte index after the token.
fn first_money(s: &str, from: usize) -> Option<(i64, usize)> {
    let b = s.as_bytes();
    let mut i = from;
    while i + 1 < b.len() {
        if b[i] == b'$' && b[i + 1].is_ascii_digit() {
            let mut j = i + 1;
            let mut dollars: i64 = 0;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b',') {
                if b[j].is_ascii_digit() {
                    dollars = dollars.saturating_mul(10).saturating_add((b[j] - b'0') as i64);
                }
                j += 1;
            }
            let mut cents: i64 = 0;
            if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
                j += 1;
                let mut k = 0;
                while k < 2 && j < b.len() && b[j].is_ascii_digit() {
                    cents = cents * 10 + (b[j] - b'0') as i64;
                    j += 1;
                    k += 1;
                }
                if k == 1 {
                    cents *= 10;
                }
            }
            return Some((dollars.saturating_mul(100).saturating_add(cents), j));
        }
        i += 1;
    }
    None
}

/// Byte index just after the first `YYYY-MM-DD` on the line.
fn date_end(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    (0..b.len().saturating_sub(9))
        .find(|&i| {
            b[i..i + 10].iter().enumerate().all(|(k, &c)| match k {
                4 | 7 => c == b'-',
                _ => c.is_ascii_digit(),
            })
        })
        .map(|i| i + 10)
}

/// `(budget, spent)` in dollars. Budget: the first money token on the first line containing "budget".
/// Spent: the first money token after the date on each dated line, summed. Undated lines don't count.
pub fn parse_budget(text: &str) -> Option<(f64, f64)> {
    let line = text
        .lines()
        .find(|l| l.to_ascii_lowercase().contains("budget"))?;
    let (budget, _) = first_money(line, 0)?;
    let mut spent: i64 = 0;
    for line in text.lines() {
        if let Some(from) = date_end(line) {
            if let Some((cents, _)) = first_money(line, from) {
                spent += cents;
            }
        }
    }
    Some((budget as f64 / 100.0, spent as f64 / 100.0))
}

/// `$DISCORD_INBOX_BUDGET` with `{month}` replaced by the message's `YYYY-MM`. Unset or empty gives `None`.
pub fn budget_path(received: &str) -> Option<PathBuf> {
    let template = std::env::var("DISCORD_INBOX_BUDGET")
        .ok()
        .filter(|v| !v.is_empty())?;
    let month = received.get(..7)?;
    Some(PathBuf::from(template.replace("{month}", month)))
}

fn money(dollars: f64) -> String {
    let cents = (dollars * 100.0).round() as i64;
    let sign = if cents < 0 { "-" } else { "" };
    let a = cents.abs();
    format!("{sign}${}.{:02}", a / 100, a % 100)
}

/// "Budget left before this: $X of $Y (YYYY-MM)" from the month's note. Any failure gives `None`.
pub fn budget_line(received: &str) -> Option<String> {
    let path = budget_path(received)?;
    let text = fs::read_to_string(path).ok()?;
    let (budget, spent) = parse_budget(&text)?;
    let month = received.get(..7)?;
    Some(format!(
        "Budget left before this: {} of {} ({month})",
        money(budget - spent),
        money(budget)
    ))
}

/// The reply text. Header lines (one per item, then the budget if a receipt was found), a blank line,
/// then the OCR text: alone for one attachment, each under `file:` for several.
pub fn reply_text(
    items: &[(String, String, Option<Insight>)],
    budget: Option<String>,
) -> String {
    let mut head: Vec<String> = Vec::new();
    for (file, _, insight) in items {
        let Some(i) = insight else { continue };
        if i.receipt {
            let total = if i.total.starts_with(|c: char| c.is_ascii_digit()) {
                format!("${}", i.total)
            } else {
                i.total.clone()
            };
            head.push(format!("Receipt: {} · {} · {}", i.merchant, total, i.category));
        } else {
            head.push(format!("Not a receipt: {file}"));
        }
    }
    let any_receipt = items
        .iter()
        .any(|(_, _, i)| i.as_ref().is_some_and(|i| i.receipt));
    if let (true, Some(b)) = (any_receipt, budget) {
        head.push(b);
    }
    let head = if head.is_empty() {
        String::new()
    } else {
        format!("{}\n\n", head.join("\n"))
    };

    let blocks: Vec<String> = items
        .iter()
        .map(|(file, text, _)| {
            if items.len() >= 2 {
                format!("{file}:\n{text}")
            } else {
                text.clone()
            }
        })
        .collect();
    let body = blocks.join("\n\n");
    let out = format!("{head}{body}");
    if out.chars().count() <= DISCORD_MAX {
        return out;
    }
    // Over the cap: keep the header, cut the OCR text, and mark the cut with an ellipsis.
    let limit = DISCORD_MAX - 1;
    let head_len = head.chars().count();
    if head_len >= limit {
        let cut: String = head.chars().take(limit).collect();
        return format!("{cut}…");
    }
    let room = limit - head_len;
    let kept: String = body.chars().take(room).collect();
    format!("{head}{kept}…")
}

/// The reply for one message, or `None` when it has no OCR text. The model and the budget are optional.
pub async fn compose(msg: &Incoming) -> Option<String> {
    if msg.ocr.is_empty() {
        return None;
    }
    let cmd = command();
    let mut items: Vec<(String, String, Option<Insight>)> = Vec::new();
    for (i, text) in &msg.ocr {
        let file = msg
            .attachments
            .get(*i)
            .map(|a| a.0.clone())
            .unwrap_or_default();
        let insight = if cmd.is_empty() {
            None
        } else {
            let (c, t) = (cmd.clone(), text.clone());
            let got = tokio::task::spawn_blocking(move || ask(&c, &t, TIMEOUT))
                .await
                .ok()
                .flatten();
            if got.is_none() {
                eprintln!(
                    "discord-inbox: no insight for attachment {} of {}",
                    i + 1,
                    msg.msg_id
                );
            }
            got
        };
        items.push((file, mask_cards(text), insight));
    }
    let any_receipt = items
        .iter()
        .any(|(_, _, i)| i.as_ref().is_some_and(|i| i.receipt));
    let budget = if any_receipt {
        budget_line(&msg.received)
    } else {
        None
    };
    Some(reply_text(&items, budget))
}
