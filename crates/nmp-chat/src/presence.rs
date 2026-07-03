//! Chat read-state and typing projection.
//!
//! The projection consumes scoped message events, read-marker advances, local
//! typing updates, remote typing events, and explicit clock updates. It never
//! reads wall clock time itself, so replay stays deterministic.

use std::collections::BTreeMap;
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::ObservedProjectionSink;
use nmp_nip29::kinds::h_tag_value;
use serde::{Deserialize, Serialize};

use crate::typing_event::ChatRemoteTypingSpec;

/// The event cursor up to which the active user has read.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadMarker {
    /// Raw hex event id at the read boundary.
    pub event_id: String,
    /// Boundary event `created_at`, Unix seconds.
    pub created_at: u64,
}

impl ReadMarker {
    #[must_use]
    pub fn new(event_id: impl Into<String>, created_at: u64) -> Self {
        Self {
            event_id: event_id.into(),
            created_at,
        }
    }

    fn is_newer_than(&self, other: &Self) -> bool {
        self.created_at > other.created_at
            || (self.created_at == other.created_at && self.event_id > other.event_id)
    }
}

/// A user currently typing in the scoped chat.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChatPresenceTyping {
    /// Raw hex pubkey.
    pub pubkey: String,
    pub updated_at_ms: u64,
    /// Explicit expiry boundary. Pruned only after a clock input advances past it.
    pub expires_at_ms: u64,
}

/// Explicit local typing input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypingUpdate {
    pub pubkey: String,
    pub is_typing: bool,
    pub updated_at_ms: u64,
    pub expires_at_ms: u64,
}

impl TypingUpdate {
    #[must_use]
    pub fn started(pubkey: impl Into<String>, updated_at_ms: u64, expires_at_ms: u64) -> Self {
        Self {
            pubkey: pubkey.into(),
            is_typing: true,
            updated_at_ms,
            expires_at_ms,
        }
    }

    #[must_use]
    pub fn stopped(pubkey: impl Into<String>, updated_at_ms: u64) -> Self {
        Self {
            pubkey: pubkey.into(),
            is_typing: false,
            updated_at_ms,
            expires_at_ms: updated_at_ms,
        }
    }
}

/// Typed chat-presence read model.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChatPresenceSnapshot {
    pub host_relay_url: String,
    pub group_id: String,
    pub active_pubkey: String,
    pub read_marker: Option<ReadMarker>,
    pub unread_count: u32,
    pub typing: Vec<ChatPresenceTyping>,
}

#[derive(Clone, Debug)]
struct MessageRow {
    id: String,
    author: String,
    created_at: u64,
}

/// Accumulates one chat scope's read marker, unread count, and typing state.
pub struct ChatPresenceProjection {
    host_relay_url: String,
    group_id: String,
    active_pubkey: String,
    message_kinds: Vec<u32>,
    remote_typing: ChatRemoteTypingSpec,
    state: Mutex<ChatPresenceState>,
}

struct ChatPresenceState {
    messages: BoundedMessageMap<String, MessageRow>,
    read_marker: Option<ReadMarker>,
    typing: BTreeMap<String, ChatPresenceTyping>,
    now_ms: u64,
}

impl ChatPresenceProjection {
    #[must_use]
    pub fn new(
        host_relay_url: impl Into<String>,
        group_id: impl Into<String>,
        active_pubkey: impl Into<String>,
        message_kinds: Vec<u32>,
    ) -> Self {
        Self::with_remote_typing(
            host_relay_url,
            group_id,
            active_pubkey,
            message_kinds,
            ChatRemoteTypingSpec::default(),
        )
    }

    #[must_use]
    pub fn with_remote_typing(
        host_relay_url: impl Into<String>,
        group_id: impl Into<String>,
        active_pubkey: impl Into<String>,
        message_kinds: Vec<u32>,
        remote_typing: ChatRemoteTypingSpec,
    ) -> Self {
        let mut kinds = message_kinds;
        kinds.sort_unstable();
        kinds.dedup();
        Self {
            host_relay_url: host_relay_url.into(),
            group_id: group_id.into(),
            active_pubkey: active_pubkey.into(),
            message_kinds: kinds,
            remote_typing,
            state: Mutex::new(ChatPresenceState {
                messages: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
                read_marker: None,
                typing: BTreeMap::new(),
                now_ms: 0,
            }),
        }
    }

    /// Advance the read cursor monotonically. Returns `true` if state changed.
    pub fn mark_read(&self, marker: ReadMarker) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        match state.read_marker.as_ref() {
            Some(current) if !marker.is_newer_than(current) => false,
            _ => {
                state.read_marker = Some(marker);
                true
            }
        }
    }

    /// Apply an explicit typing update. Expiration is driven by explicit clock
    /// inputs, not wall-clock reads inside this projection.
    pub fn apply_typing(&self, update: TypingUpdate) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        state.now_ms = state.now_ms.max(update.updated_at_ms);
        state.prune_expired();
        if update.pubkey.is_empty() || update.pubkey == self.active_pubkey {
            return false;
        }
        if update.is_typing && update.expires_at_ms > state.now_ms {
            state.typing.insert(
                update.pubkey.clone(),
                ChatPresenceTyping {
                    pubkey: update.pubkey,
                    updated_at_ms: update.updated_at_ms,
                    expires_at_ms: update.expires_at_ms,
                },
            );
            true
        } else {
            state.typing.remove(&update.pubkey).is_some()
        }
    }

    /// Explicitly advance the projection clock and prune expired typing rows.
    pub fn advance_clock(&self, now_ms: u64) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        state.now_ms = state.now_ms.max(now_ms);
        state.prune_expired()
    }

    #[must_use]
    pub fn snapshot(&self) -> ChatPresenceSnapshot {
        self.state
            .lock()
            .map(|state| state.snapshot(self))
            .unwrap_or_else(|_| ChatPresenceSnapshot {
                host_relay_url: self.host_relay_url.clone(),
                group_id: self.group_id.clone(),
                active_pubkey: self.active_pubkey.clone(),
                ..Default::default()
            })
    }

    fn same_group(&self, event: &KernelEvent) -> bool {
        h_tag_value(&event.tags) == Some(self.group_id.as_str())
    }

    fn accepts_message(&self, event: &KernelEvent) -> bool {
        self.same_group(event)
            && !self.remote_typing.contains_kind(event.kind)
            && (self.message_kinds.is_empty() || self.message_kinds.contains(&event.kind))
    }
}

impl ObservedProjectionSink for ChatPresenceProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !self.same_group(event) {
            return;
        }
        if let Some(update) = self.remote_typing.update_from_event(event) {
            let _ = self.apply_typing(update);
            return;
        }
        if !self.accepts_message(event) {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.messages.insert(
            event.id.clone(),
            MessageRow {
                id: event.id.clone(),
                author: event.author.clone(),
                created_at: event.created_at,
            },
        );
    }
}

impl ChatPresenceState {
    fn snapshot(&self, projection: &ChatPresenceProjection) -> ChatPresenceSnapshot {
        ChatPresenceSnapshot {
            host_relay_url: projection.host_relay_url.clone(),
            group_id: projection.group_id.clone(),
            active_pubkey: projection.active_pubkey.clone(),
            read_marker: self.read_marker.clone(),
            unread_count: self.unread_count(&projection.active_pubkey),
            typing: self.typing.values().cloned().collect(),
        }
    }

    fn unread_count(&self, active_pubkey: &str) -> u32 {
        let count = self
            .messages
            .values()
            .filter(|m| m.author != active_pubkey)
            .filter(|m| {
                self.read_marker
                    .as_ref()
                    .is_none_or(|r| message_after(m, r))
            })
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    fn prune_expired(&mut self) -> bool {
        let before = self.typing.len();
        self.typing
            .retain(|_, typing| typing.expires_at_ms > self.now_ms);
        before != self.typing.len()
    }
}

fn message_after(message: &MessageRow, marker: &ReadMarker) -> bool {
    message.created_at > marker.created_at
        || (message.created_at == marker.created_at && message.id > marker.event_id)
}

#[cfg(test)]
#[path = "presence_tests.rs"]
mod tests;
