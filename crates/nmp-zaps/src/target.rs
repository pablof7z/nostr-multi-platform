//! `ZapTarget` — a validated raw NIP-57 zap target for [`crate::open_zaps`].
//!
//! Scope is a plain kind:1 note event id (#2758's "plain notes" slice); a
//! sibling `ZapTarget::address` for addressable (kind:3xxxx) targets is a
//! natural follow-up, out of scope here.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ZapTarget {
    event_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ZapTargetError {
    InvalidEventId,
}

impl core::fmt::Display for ZapTargetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidEventId => write!(f, "zap target must be a 64-hex event id"),
        }
    }
}

impl std::error::Error for ZapTargetError {}

impl ZapTargetError {
    /// The stable machine code (crosses the wire as an FFI error code; never
    /// renumbered or repurposed — #2899 Part A).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidEventId => "invalid_event_id",
        }
    }
}

impl ZapTarget {
    /// Validate `event_id` as a 64-hex kind:1 note id.
    pub fn event(event_id: impl Into<String>) -> Result<Self, ZapTargetError> {
        let event_id = event_id.into().trim().to_string();
        if !is_hex64(&event_id) {
            return Err(ZapTargetError::InvalidEventId);
        }
        Ok(Self { event_id })
    }

    /// The raw hex event id this target's zap summary is keyed by.
    #[must_use]
    pub fn event_id(&self) -> &str {
        &self.event_id
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
