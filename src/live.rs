//! Serenity adapter shared by the bots: one message and the DM history calls, as the lib's `Inbound` and `History`.
use crate::ingest::{History, Inbound, PAGE};
use crate::rich::Rich;
use serenity::all::{
    ChannelId, CreateAttachment, CreateEmbed, CreateEmbedFooter, CreateMessage, GetMessages, Http, Message,
    MessageId, ReactionType, UserId,
};
use std::sync::Arc;

/// First `n` characters of `s` (chars, not bytes, so no UTF-8 split).
fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Sends a `Rich` as an embed (plus its image as an attachment) into `channel`.
/// Discord limits: title 256, description 4000, footer 2048, field name 256, field value 1024, 25 fields.
/// If the embed send fails, falls back to plain text (2000 chars).
pub async fn send_rich(http: &Arc<Http>, channel: ChannelId, r: &Rich) -> Result<(), String> {
    let mut embed = CreateEmbed::new().colour(r.color);
    if !r.title.is_empty() {
        embed = embed.title(cut(&r.title, 256));
    }
    if !r.description.is_empty() {
        embed = embed.description(cut(&r.description, 4000));
    }
    if !r.footer.is_empty() {
        embed = embed.footer(CreateEmbedFooter::new(cut(&r.footer, 2048)));
    }
    for (name, value, inline) in r.fields.iter().take(25) {
        let value = if value.trim().is_empty() { "—".to_string() } else { cut(value, 1024) };
        embed = embed.field(cut(name, 256), value, *inline);
    }
    let mut msg = CreateMessage::new();
    if let Some((name, bytes)) = &r.image {
        embed = embed.image(format!("attachment://{name}"));
        msg = msg.add_file(CreateAttachment::bytes(bytes.clone(), name.clone()));
    }
    match channel.send_message(http, msg.embed(embed)).await {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("discord-inbox: embed send to {channel} failed, sending text: {e}");
            channel
                .say(http, cut(&r.text, 2000))
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    }
}

/// One serenity message, adapted to the lib's `Inbound`.
pub struct LiveMsg {
    pub http: Arc<Http>,
    pub msg: Message,
    /// false for a message fetched from history by the catch-up (#45).
    pub live: bool,
}

impl Inbound for LiveMsg {
    fn author_id(&self) -> u64 {
        self.msg.author.id.get()
    }
    fn msg_id(&self) -> u64 {
        self.msg.id.get()
    }
    fn channel_id(&self) -> u64 {
        self.msg.channel_id.get()
    }
    fn received(&self) -> String {
        crate::when::local_stamp(*self.msg.timestamp)
    }
    fn content(&self) -> String {
        let snaps: Vec<String> = self.msg.message_snapshots.iter().map(|s| s.content.clone()).collect();
        crate::ingest::forwarded_text(&self.msg.content, &snaps)
    }
    fn attachment_names(&self) -> Vec<String> {
        self.msg
            .attachments
            .iter()
            .map(|a| a.filename.clone())
            .collect()
    }
    async fn download(&self, i: usize) -> Result<Vec<u8>, String> {
        self.msg.attachments[i]
            .download()
            .await
            .map_err(|e| e.to_string())
    }
    async fn react(&self, emoji: &str) -> Result<(), String> {
        self.msg
            .react(&self.http, ReactionType::Unicode(emoji.to_string()))
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    async fn unreact(&self, emoji: &str) -> Result<(), String> {
        self.msg
            .delete_reaction(&self.http, None, ReactionType::Unicode(emoji.to_string()))
            .await
            .map_err(|e| e.to_string())
    }
    fn own_reactions(&self) -> Vec<String> {
        self.msg
            .reactions
            .iter()
            .filter(|r| r.me)
            .filter_map(|r| match &r.reaction_type {
                ReactionType::Unicode(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
    async fn reply(&self, text: &str) -> Result<(), String> {
        self.msg
            .channel_id
            .say(&self.http, text)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    async fn reply_rich(&self, r: &Rich) -> Result<(), String> {
        send_rich(&self.http, self.msg.channel_id, r).await
    }
    fn live(&self) -> bool {
        self.live
    }
    async fn typing(&self) {
        if let Err(e) = self.msg.channel_id.broadcast_typing(&self.http).await {
            eprintln!("discord-inbox: typing in {} failed: {e}", self.msg.channel_id);
        }
    }
}

/// The Discord history calls, adapted to the lib's `History`.
pub struct Live {
    pub http: Arc<Http>,
}

impl History for Live {
    type Msg = LiveMsg;

    async fn dm_channel(&self, user_id: u64) -> Result<u64, String> {
        UserId::new(user_id)
            .create_dm_channel(&self.http)
            .await
            .map(|c| c.id.get())
            .map_err(|e| e.to_string())
    }

    async fn page(&self, channel: u64, after: Option<u64>) -> Result<Vec<LiveMsg>, String> {
        let mut req = GetMessages::new().limit(PAGE as u8);
        if let Some(a) = after {
            req = req.after(MessageId::new(a));
        }
        let msgs = ChannelId::new(channel)
            .messages(&self.http, req)
            .await
            .map_err(|e| e.to_string())?;
        Ok(msgs
            .into_iter()
            .map(|msg| LiveMsg {
                http: self.http.clone(),
                msg,
                live: false,
            })
            .collect())
    }

    async fn fetch(&self, channel: u64, msg_id: u64) -> Result<LiveMsg, String> {
        let msg = ChannelId::new(channel)
            .message(&self.http, MessageId::new(msg_id))
            .await
            .map_err(|e| e.to_string())?;
        Ok(LiveMsg {
            http: self.http.clone(),
            msg,
            live: false,
        })
    }
}

