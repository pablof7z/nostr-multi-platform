/// Display-only reaction count for a Rust-owned group-chat message row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NostrGroupChatReactionWire {
    pub emoji: String,
    pub count: u32,
}

impl NostrGroupChatReactionWire {
    pub fn label(&self) -> String {
        if self.count <= 1 {
            self.emoji.clone()
        } else {
            format!("{} {}", self.emoji, self.count)
        }
    }
}

/// Display-only group-chat message row projected by Rust.
///
/// TUI components render this mirror as-is. They do not parse Nostr tags,
/// decide relay policy, publish replies, or derive reaction/read state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NostrGroupChatMessageWire {
    pub id: String,
    pub author_pubkey: String,
    pub content: String,
    pub created_at_label: String,
    pub reply_preview: Option<String>,
    pub reactions: Vec<NostrGroupChatReactionWire>,
    pub is_outgoing: bool,
}

impl NostrGroupChatMessageWire {
    pub fn reply_preview(&self) -> Option<&str> {
        non_empty(self.reply_preview.as_deref())
    }
}

/// Display-only group participant row projected by Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NostrGroupChatParticipantWire {
    pub pubkey: String,
    pub role_label: Option<String>,
    pub status_label: Option<String>,
}

impl NostrGroupChatParticipantWire {
    pub fn role_label(&self) -> Option<&str> {
        non_empty(self.role_label.as_deref())
    }

    pub fn status_label(&self) -> Option<&str> {
        non_empty(self.status_label.as_deref())
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
