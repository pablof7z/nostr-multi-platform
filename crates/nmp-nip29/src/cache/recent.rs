//! `previous`-tag prefix helper (`moderation.md` §2 quoted text).
//!
//! The per-group recent-events LRU that used to live here was retired: the
//! kernel event store already indexes every ingested event by its single-letter
//! `h` tag, so `previous` tags are derived at publish time from a cache-only
//! `StoreQuery::Tags { #h, limit }` read (see `action::group_publish`) — a single
//! source of truth instead of a parallel crate-local cache.

/// First 8 hex characters of an event id (per `moderation.md` §2 quoted text).
pub type EventIdPrefix = String;

/// Truncate a hex event id to its first 8 chars for `previous`-tag emission.
#[must_use]
pub fn previous_tag_prefix(event_id: &str) -> EventIdPrefix {
    event_id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_takes_first_8_chars() {
        assert_eq!(previous_tag_prefix("0123456789abcdef"), "01234567");
    }

    #[test]
    fn prefix_shorter_than_8_is_unchanged() {
        assert_eq!(previous_tag_prefix("abc"), "abc");
    }
}
