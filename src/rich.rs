//! U5: the rich reply type (embed-shaped, with plain-text fallback). Filled in by unit U5.

/// A reply shaped like a Discord embed. `text` is the plain fallback, sent when the embed can't be.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rich {
    pub text: String,
    pub title: String,
    pub description: String,
    /// (name, value, inline)
    pub fields: Vec<(String, String, bool)>,
    pub color: u32,
    pub footer: String,
    /// (file name, PNG bytes); the embed shows it as `attachment://{name}`.
    pub image: Option<(String, Vec<u8>)>,
}
