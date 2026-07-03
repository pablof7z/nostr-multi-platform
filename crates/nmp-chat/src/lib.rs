//! `nmp-chat` — reusable chat read-state primitives for NMP apps.
//!
//! This crate owns chat-screen state that is not a NIP-29 transport concern:
//! read markers, unread counts, and typing participants. Group routing stays
//! composed through `GroupId` and host-pinned `#h` filters; apps opt into
//! remote typing by declaring the event kind they publish through NIP-29.

mod group;
pub mod ownership;
mod presence;
mod typing_event;
mod wire;

pub use group::{
    chat_presence_filter_json, chat_presence_filter_json_with_remote_typing,
    chat_presence_projection_key, close_chat_presence_session, open_chat_presence_session,
    open_chat_presence_session_with_reader, ChatPresenceHandle, ChatPresenceSession,
    CHAT_PRESENCE_KEY,
};
pub use presence::{
    ChatPresenceProjection, ChatPresenceSnapshot, ChatPresenceTyping, ReadMarker, TypingUpdate,
};
pub use typing_event::{
    chat_typing_status_tag, ChatRemoteTypingSpec, CHAT_TYPING_STARTED, CHAT_TYPING_STATUS_TAG,
    CHAT_TYPING_STOPPED, DEFAULT_REMOTE_TYPING_TTL_MS,
};
pub use wire::{
    decode_chat_presence_snapshot, encode_chat_presence_snapshot, CHAT_PRESENCE_FILE_IDENTIFIER,
    CHAT_PRESENCE_SCHEMA_ID, CHAT_PRESENCE_SCHEMA_VERSION,
};
