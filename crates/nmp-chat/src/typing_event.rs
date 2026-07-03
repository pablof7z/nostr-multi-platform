//! Remote typing event contract for chat presence.
//!
//! NIP-29 carries the `h` envelope and relay pin; `nmp-chat` owns this
//! reusable typing status tag contract for apps that choose an event kind.

use nmp_core::substrate::KernelEvent;

use crate::presence::TypingUpdate;

pub const CHAT_TYPING_STATUS_TAG: &str = "typing";
pub const CHAT_TYPING_STARTED: &str = "started";
pub const CHAT_TYPING_STOPPED: &str = "stopped";
pub const DEFAULT_REMOTE_TYPING_TTL_MS: u64 = 8_000;

/// Caller-declared remote typing event shape.
///
/// The kind number remains app/protocol-owned. Events whose kind is listed here
/// must carry `["typing", "started"]` or `["typing", "stopped"]`; the
/// NIP-29 publish path injects the `h` group tag separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatRemoteTypingSpec {
    kinds: Vec<u32>,
    ttl_ms: u64,
}

impl ChatRemoteTypingSpec {
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            kinds: Vec::new(),
            ttl_ms: DEFAULT_REMOTE_TYPING_TTL_MS,
        }
    }

    #[must_use]
    pub fn new(kinds: Vec<u32>) -> Self {
        Self::with_ttl_ms(kinds, DEFAULT_REMOTE_TYPING_TTL_MS)
    }

    #[must_use]
    pub fn with_ttl_ms(kinds: Vec<u32>, ttl_ms: u64) -> Self {
        let mut kinds = kinds;
        kinds.sort_unstable();
        kinds.dedup();
        Self { kinds, ttl_ms }
    }

    #[must_use]
    pub fn kinds(&self) -> &[u32] {
        &self.kinds
    }

    #[must_use]
    pub fn contains_kind(&self, kind: u32) -> bool {
        self.kinds.binary_search(&kind).is_ok()
    }

    #[must_use]
    pub fn update_from_event(&self, event: &KernelEvent) -> Option<TypingUpdate> {
        if !self.contains_kind(event.kind) {
            return None;
        }
        let is_typing = typing_state(&event.tags)?;
        let updated_at_ms = event.created_at.saturating_mul(1_000);
        Some(if is_typing {
            TypingUpdate::started(
                event.author.clone(),
                updated_at_ms,
                updated_at_ms.saturating_add(self.ttl_ms),
            )
        } else {
            TypingUpdate::stopped(event.author.clone(), updated_at_ms)
        })
    }
}

impl Default for ChatRemoteTypingSpec {
    fn default() -> Self {
        Self::disabled()
    }
}

#[must_use]
pub fn chat_typing_status_tag(is_typing: bool) -> Vec<String> {
    vec![
        CHAT_TYPING_STATUS_TAG.to_string(),
        if is_typing {
            CHAT_TYPING_STARTED
        } else {
            CHAT_TYPING_STOPPED
        }
        .to_string(),
    ]
}

fn typing_state(tags: &[Vec<String>]) -> Option<bool> {
    tags.iter().find_map(|tag| match (tag.first(), tag.get(1)) {
        (Some(name), Some(value)) if name == CHAT_TYPING_STATUS_TAG => match value.as_str() {
            CHAT_TYPING_STARTED => Some(true),
            CHAT_TYPING_STOPPED => Some(false),
            _ => None,
        },
        _ => None,
    })
}
