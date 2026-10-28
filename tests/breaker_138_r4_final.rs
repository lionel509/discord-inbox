// Breaker round 4 on the final fix (7b31f74): the `lost` guard in daily::voice.
use discord_inbox::chat::Chat;
use discord_inbox::daily::{self, Slot};

struct Fake(String);
impl Chat for Fake {
    fn ask(&self, _system: &str, _user: &str) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

// Night facts for an 11-item list. Item 1 and item 11 share the text "rent", so "1. rent"
// is a substring of "11. rent check".
fn facts_11() -> String {
    let mut lines = vec!["Tomorrow first: nothing on the calendar".to_string(), "Todo, numbered:".to_string()];
    lines.push("1. rent".to_string());
    for k in 2..=10 {
        lines.push(format!("{k}. item number {k} on the list"));
    }
    lines.push("11. rent check".to_string());
    lines.join("\n")
}

// The model drops line "1. rent" and keeps "11. rent check". The guard must notice that
// item 1 is missing from the DM and send the plain facts.
#[test]
fn breaker_dropped_item_1_is_not_hidden_by_item_11_substring() {
    let facts = facts_11();
    let mut reply = String::from("Good night Lionel. Here is what is still open tonight:\n");
    for k in 2..=10 {
        reply.push_str(&format!("{k}. item number {k} on the list\n"));
    }
    reply.push_str("11. rent check\nWhich numbered items did you finish, and what should carry to tomorrow?");
    let got = daily::voice(&Fake(reply.clone()), Slot::Night, &facts);
    assert_eq!(got, facts, "item 1 'rent' is missing from the DM but the guard let the reply through: {got:?}");
}
