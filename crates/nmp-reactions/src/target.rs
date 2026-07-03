//! `ReactionTarget` — the raw event id a reaction read targets (#2758).
//!
//! NIP-25 reactions always target a single event id via a bare `e` tag (the
//! LAST `e` tag is the reacted-to event); there is no addressable/external/
//! comment target shape to compose the way `nmp-replies`' `ReplyTarget` must.
//! This crate therefore keeps one small, validated newtype rather than
//! reusing that multi-shape enum.

use serde::{Deserialize, Serialize};

/// A validated 64-hex reaction target event id.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReactionTarget(String);

/// Why a candidate reaction target was rejected.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReactionTargetError {
    /// The target string is not a 64-hex event id.
    InvalidEventId,
}

impl core::fmt::Display for ReactionTargetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidEventId => write!(f, "reaction target must be a 64-hex event id"),
        }
    }
}

impl std::error::Error for ReactionTargetError {}

impl ReactionTargetError {
    /// The stable machine code (crosses the wire as an FFI error code; never
    /// renumbered or repurposed — #2899 Part A).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidEventId => "invalid_event_id",
        }
    }
}

impl ReactionTarget {
    /// Validate `event_id` as a reaction target (a bare 64-hex event id).
    ///
    /// # Errors
    ///
    /// Returns [`ReactionTargetError::InvalidEventId`] when `event_id` is not
    /// 64 hex characters after trimming.
    pub fn event(event_id: impl Into<String>) -> Result<Self, ReactionTargetError> {
        let event_id = event_id.into().trim().to_string();
        if !is_hex64(&event_id) {
            return Err(ReactionTargetError::InvalidEventId);
        }
        Ok(Self(event_id))
    }

    /// Borrow the raw hex event id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume into the owned raw hex event id.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
