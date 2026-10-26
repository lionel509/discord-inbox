//! Serenity adapter shared by the bots: one message and the DM history calls, as the lib's `Inbound` and `History`.
use crate::ingest::{History, Inbound, PAGE};
use serenity::all::{ChannelId, GetMessages, Http, Message, MessageId, ReactionType, UserId};
use std::sync::Arc;

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

