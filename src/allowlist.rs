use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct Allowlist(HashSet<u64>);

pub fn parse(_text: &str) -> Result<Allowlist, String> {
    todo!()
}

impl Allowlist {
    pub fn load(path: &Path) -> Result<Allowlist, String> {
        let _ = path;
        todo!()
    }

    pub fn contains(&self, _user_id: u64) -> bool {
        todo!()
    }
}

pub fn default_path() -> PathBuf {
    todo!()
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
    fn empty_file_is_err() {
        assert!(parse("").is_err());
        assert!(parse("# nothing\n").is_err());
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
        for fake in [MAIN, ALT] {
            for x in [fake - 1, fake + 1] {
                assert!(!a.contains(x), "neighbour {x} passed");
            }
        }
    }
}
