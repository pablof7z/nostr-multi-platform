use serde::{Deserialize, Serialize};

/// The plain kind:1 note event this crate's active read counts NIP-18
/// reposts against (#2758). Scoped to plain notes for now — a generic-repost
/// (non-kind:1) target is future work, tracked by the same issue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepostTarget {
    pub(crate) event_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RepostTargetError {
    InvalidEventId,
}

impl core::fmt::Display for RepostTargetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidEventId => write!(f, "repost target must be a 64-hex event id"),
        }
    }
}

impl std::error::Error for RepostTargetError {}

impl RepostTarget {
    /// Build a repost target for the plain kind:1 note `event_id`.
    ///
    /// # Errors
    ///
    /// Returns [`RepostTargetError::InvalidEventId`] when `event_id` is not a
    /// 64-hex Nostr event id.
    pub fn note(event_id: impl Into<String>) -> Result<Self, RepostTargetError> {
        let event_id = event_id.into().trim().to_string();
        if !is_hex64(&event_id) {
            return Err(RepostTargetError::InvalidEventId);
        }
        Ok(Self { event_id })
    }

    /// The raw target event id.
    #[must_use]
    pub fn event_id(&self) -> &str {
        &self.event_id
    }
}

pub(crate) fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_hex_event_id() {
        assert_eq!(
            RepostTarget::note("not-hex"),
            Err(RepostTargetError::InvalidEventId)
        );
    }

    #[test]
    fn trims_and_accepts_valid_event_id() {
        let id = "a".repeat(64);
        let target = RepostTarget::note(format!("  {id}  ")).unwrap();
        assert_eq!(target.event_id(), id);
    }
}
