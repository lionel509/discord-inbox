//! The agentic lookup loop (#49): the model may call the read-only note tools a few times,
//! then answers. Every limit is here: the number of calls, the characters of tool output per
//! DM, and the wall clock. The deadline is checked only before a call starts, so the worst case is about
//! DEADLINE + two model timeouts (~100 s), not DEADLINE itself. Once one is hit, the last model call has its tools switched off.
use crate::chat::{Chat, Turn, Usage};
use crate::tools::Lookup;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const MAX_CALLS: usize = 6;
pub const BUDGET_CHARS: usize = 30_000;
pub const DEADLINE: Duration = Duration::from_secs(40);
pub const LIMIT_NOTE: &str = "(That was your last lookup. Answer now from what you've read, say warmly that you couldn't check everything, and don't call any tool.)";

const CUT_BUDGET: &str = "…[cut: budget]";

#[derive(Debug)]
pub struct Answered {
    pub text: String,
    pub calls: usize,
    pub usage: Usage,
    /// Why the tools were switched off: `lookups`, `budget` or `time`. `None` when the model answered on its own.
    pub limited: Option<&'static str>,
}

/// Answers one question by letting the model use `lookup` until it has an answer.
pub fn answer(chat: &dyn Chat, lookup: &Lookup, question: &str) -> Result<Answered, String> {
    let (system, user) = crate::vaults::answer_prompt(question, &[]);
    let mut messages = vec![
        json!({"role": "system", "content": system}),
        json!({"role": "user", "content": user}),
    ];
    let tools = crate::tools::schema();
    let start = Instant::now();
    let mut calls = 0usize;
    let mut used = 0usize;
    let mut usage = Usage::default();
    let mut limited: Option<&'static str> = None;
    loop {
        let allow = limited.is_none();
        let mut reply = chat.ask_tools(&messages, &tools, allow)?;
        if !allow && matches!(reply.turn, Turn::Calls(_)) {
            // The provider ignored tool_choice "none": ask once more with the tools key left out.
            let again = chat.ask_tools(&messages, &[], false)?;
            usage.prompt += reply.usage.prompt;
            usage.completion += reply.usage.completion;
            reply = again;
        }
        usage.prompt += reply.usage.prompt;
        usage.completion += reply.usage.completion;
        match reply.turn {
            Turn::Text(text) => return Ok(Answered { text, calls, usage, limited }),
            Turn::Calls(_) if !allow => {
                let text = "I couldn't check everything I wanted to, so I can't give a full answer.".to_string();
                return Ok(Answered { text, calls, usage, limited });
            }
            Turn::Calls(calls_in_round) => {
                let tool_calls: Vec<Value> = calls_in_round
                    .iter()
                    .map(|c| {
                        json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}})
                    })
                    .collect();
                messages.push(json!({"role": "assistant", "content": null, "tool_calls": tool_calls}));
                for c in &calls_in_round {
                    // Parallel calls in one turn count too: past the cap, answer the id without running it.
                    let result = if calls >= MAX_CALLS || start.elapsed() >= DEADLINE {
                        "error: lookup limit reached".to_string()
                    } else {
                        calls += 1;
                        match lookup.call(&c.name, &c.arguments) {
                            Ok(s) => s,
                            Err(e) => format!("error: {e}"),
                        }
                    };
                    let content = fit(&result, BUDGET_CHARS.saturating_sub(used));
                    used += content.chars().count();
                    messages.push(json!({"role": "tool", "tool_call_id": c.id, "content": content}));
                }
                let next = if calls >= MAX_CALLS {
                    Some("lookups")
                } else if used >= BUDGET_CHARS {
                    Some("budget")
                } else if start.elapsed() >= DEADLINE {
                    Some("time")
                } else {
                    None
                };
                if limited.is_none() && next.is_some() {
                    messages.push(json!({"role": "user", "content": LIMIT_NOTE}));
                }
                limited = limited.or(next);
            }
        }
    }
}

/// `s` if it fits in `room` characters, else cut to fit with the budget marker at the end.
fn fit(s: &str, room: usize) -> String {
    if s.chars().count() <= room {
        return s.to_string();
    }
    let mark = CUT_BUDGET.chars().count();
    if room < mark {
        return CUT_BUDGET.chars().take(room).collect();
    }
    let mut out: String = s.chars().take(room - mark).collect();
    out.push_str(CUT_BUDGET);
    out
}
