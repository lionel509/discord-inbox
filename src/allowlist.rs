use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct Allowlist(HashSet<u64>);

/// One user ID per line. Blank lines and `#` comments are skipped.
pub fn parse(text: &str) -> Result<Allowlist, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut ids = HashSet::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let id: u64 = line
            .parse()
            .map_err(|_| format!("allowed_ids: bad line {}", n + 1))?;
        if id == 0 {
            return Err("allowed_ids: 0 is not a user ID".into());
        }
        ids.insert(id);
    }
    if ids.is_empty() {
        return Err("allowed_ids: no user IDs listed".into());
    }
    Ok(Allowlist(ids))
}

impl Allowlist {
    pub fn load(path: &Path) -> Result<Allowlist, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("allowed_ids not found at {}: {e}", path.display()))?;
        parse(&text)
    }

    pub fn contains(&self, user_id: u64) -> bool {
        self.0.contains(&user_id)
    }

    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.0.iter().copied()
    }
}

/// `$DISCORD_INBOX_ALLOWED_IDS_FILE`, else `~/.config/discord-inbox/allowed_ids`.
pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DISCORD_INBOX_ALLOWED_IDS_FILE") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/discord-inbox/allowed_ids")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const MAIN: u64 = 100000000000000001;
    const ALT: u64 = 100000000000000002;
    const STRANGER: u64 = 999999999999999999;

    fn two_ids() -> Allowlist {
        parse("100000000000000001\n100000000000000002\n").unwrap()
    }

    #[test]
    fn both_configured_ids_pass() {
        let a = two_ids();
        assert!(a.contains(MAIN));
        assert!(a.contains(ALT));
    }

    #[test]
    fn other_id_is_rejected() {
        let a = two_ids();
        assert!(!a.contains(STRANGER));
        assert!(!a.contains(0));
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let a = parse("# main\n100000000000000001\n\n100000000000000002\n").unwrap();
        assert!(a.contains(MAIN) && a.contains(ALT));
        assert!(!a.contains(STRANGER));
    }

    #[test]
    fn trailing_comment_on_id_line_parses() {
        let a = parse("100000000000000001 # main\n100000000000000002 # alt\n").unwrap();
        assert!(a.contains(MAIN) && a.contains(ALT));
    }

    #[test]
    fn bom_prefixed_allowlist_parses() {
        let a = parse("\u{feff}100000000000000001\n100000000000000002\n").unwrap();
        assert!(a.contains(MAIN) && a.contains(ALT));
    }

    #[test]
    fn empty_file_is_err() {
        assert!(parse("").is_err());
        assert!(parse("# nothing\n").is_err());
    }

    #[test]
    fn zero_is_err() {
        assert!(parse("0\n").is_err());
        assert!(parse("100000000000000001\n0\n").is_err());
    }

    #[test]
    fn junk_line_is_err() {
        assert!(parse("100000000000000001\nlionel\n").is_err());
    }

    #[test]
    fn missing_file_is_err() {
        let err = match Allowlist::load(Path::new("/nonexistent/allowed_ids")) {
            Ok(_) => panic!("expected Err"),
            Err(e) => e,
        };
        assert!(err.contains("allowed_ids"), "{err}");
    }

    #[test]
    fn other_ids_never_pass() {
        let a = two_ids();
        let mut state: u64 = 42;
        for _ in 0..10_000 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            if state != MAIN && state != ALT {
                assert!(!a.contains(state), "random id {state} passed");
            }
        }
        // ALT is MAIN + 1, so only neighbours that are not themselves allowed apply.
        for fake in [MAIN, ALT] {
            for x in [fake - 1, fake + 1] {
                if x != MAIN && x != ALT {
                    assert!(!a.contains(x), "neighbour {x} passed");
                }
            }
        }
    }
}
