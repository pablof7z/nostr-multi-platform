//! `ThreadPointer` — what a reply / comment is anchored to.
//!
//! Three variants cover every NIP-10 / NIP-22 anchor shape: a Nostr event id,
//! a parameterized-replaceable address, or an external URI.

use serde::{Deserialize, Serialize};

/// Anchor for a reply / comment. Address and External variants terminate
/// ancestor-walk: there is no event id to hydrate.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ThreadPointer {
    Event {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<u32>,
    },
    Address {
        coord: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<u32>,
    },
    External {
        uri: String,
    },
}

impl ThreadPointer {
    /// Event id when this pointer names a specific event; `None` for
    /// `Address` and `External` (they terminate ancestor-walk).
    #[must_use] 
    pub fn event_id(&self) -> Option<&str> {
        match self {
            Self::Event { id, .. } => Some(id.as_str()),
            _ => None,
        }
    }
}
