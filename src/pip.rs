//! Pip's handler (U4). Stub: functions are todo!().
use crate::allowlist::Allowlist;
use crate::daylog::Totals;
use crate::ingest::{Handle, Inbound, Outcome};
use crate::nutrition::Parsed;
use std::path::PathBuf;

/// Pip's public presence: persona only. Set in U4.
pub const PRESENCE: &str = "";

#[derive(Clone, Debug)]
pub struct Config {
    pub nutrition_dir: PathBuf,
    pub recipes_dir: Option<PathBuf>,
    pub low_kcal: u32,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        todo!()
    }
}

pub async fn handle<M: Inbound>(_allow: &Allowlist, _cfg: &Config, _m: &M) -> Outcome {
    todo!()
}

pub struct PipHandler<'a> {
    pub allow: &'a Allowlist,
    pub cfg: &'a Config,
}

impl<M: Inbound> Handle<M> for PipHandler<'_> {
    fn handle(&self, m: &M) -> impl std::future::Future<Output = Outcome> + Send {
        async move {
            let _ = m;
            todo!()
        }
    }
}

pub fn reply_text(
    _parsed: &Parsed,
    _recipe: Option<(&str, f64)>,
    _overridden: &[String],
    _skipped: &[String],
    _today: Option<&Totals>,
) -> String {
    todo!()
}
