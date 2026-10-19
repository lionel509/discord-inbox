//! Stormy's typed DM commands (#92): `/commands` and `/clear`. Plain text, matched exactly; not Discord slash commands.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slash {
    Commands,
    Clear,
}

/// The whole message, trimmed and lowercased, must be one of the commands. Anything else goes on to the other handlers.
pub fn parse(content: &str) -> Option<Slash> {
    match content.trim().to_lowercase().as_str() {
        "/commands" => Some(Slash::Commands),
        "/clear" => Some(Slash::Clear),
        _ => None,
    }
}

/// Blank lines posted by `/clear`. Discord rejects blank replies, so each line is `_ _`, which renders as an empty line.
pub const CLEAR_LINES: usize = 40;

pub fn clear_text() -> String {
    vec!["_ _"; CLEAR_LINES].join("\n")
}

pub const COMMANDS_TEXT: &str = "**Stormy commands**
`/commands` — this list
`/clear` — scroll the chat clear and forget our last few messages
`restock [all] [YYYY-MM-DD]` — add a grocery trip to the pantry
`out of <food>` — drop it from the pantry
Anything else: I save it, file it, and reply.";
