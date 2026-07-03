//! `nmp-chat` — reusable chat read-state primitives for NMP apps.
//!
//! This crate owns chat-screen state that is not a NIP-29 transport concern:
//! read markers, unread counts, and typing participants. Group routing is
//! composed through `GroupId` and host-pinned `#h` filters, but the projection
//! never names chat event kind constants or publishes events.

mod group;
pub mod ownership;
mod presence;
mod wire;

pub use group::{
    chat_presence_filter_json, chat_presence_projection_key, close_chat_presence_session,
    open_chat_presence_session, open_chat_presence_session_with_reader, ChatPresenceHandle,
    ChatPresenceSession, CHAT_PRESENCE_KEY,
};
pub use presence::{
    ChatPresenceProjection, ChatPresenceSnapshot, ChatPresenceTyping, ReadMarker, TypingUpdate,
};
pub use wire::{
    decode_chat_presence_snapshot, encode_chat_presence_snapshot, CHAT_PRESENCE_FILE_IDENTIFIER,
    CHAT_PRESENCE_SCHEMA_ID, CHAT_PRESENCE_SCHEMA_VERSION,
};
