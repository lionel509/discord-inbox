//! Stormy's persona in the chat and answer prompts, and the name-spelling note (#45). Pure prompt text.
use discord_inbox::vaults::{answer_prompt, system_prompt};

const PERSONA_HEAD: &str = "You're Stormy, Pusheen's little sister: a fluffy gray cat and Lionel's personal assistant.";
const PERSONA_TAIL: &str = "When something fails, say so plainly.";
const PERSONA_WARM: &str = "You genuinely care about Lionel and you're on his side: be warm and encouraging, never cold or robotic, even when something went wrong.";

#[test]
fn chat_prompt_carries_the_persona() {
    let s = system_prompt();
    assert!(s.contains(PERSONA_HEAD), "{s}");
    assert!(s.contains(PERSONA_TAIL), "{s}");
    assert!(s.contains(PERSONA_WARM), "{s}");
}

#[test]
fn answer_prompt_carries_the_persona_and_the_name_note() {
    let (system, _) = answer_prompt("how much does willam owe me", &[]);
    assert!(system.contains(PERSONA_HEAD), "{system}");
    assert!(system.contains(PERSONA_TAIL), "{system}");
    assert!(system.contains(PERSONA_WARM), "{system}");
    assert!(system.contains("misspelled") && system.contains("\"willam\" = William"), "{system}");
}
