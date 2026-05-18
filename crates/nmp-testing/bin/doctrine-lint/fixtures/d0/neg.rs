//! Negative D0 fixture — must produce zero findings.
//!
//! Uses doc-comment "NIP-29" references (allowed), generic substrate types
//! like `RelayUrl`, and unrelated identifiers. Verb forms of "group" (e.g.
//! "group by") are explicitly allowed by the rule.

/// Example use case: NIP-29 relay-based groups. This doc-comment text is
/// allowed because `Nip29`-flavoured tokens here are explanatory prose, not
/// kernel types.
pub fn pin_subscription_to_relay(relay_pin: &str) -> bool {
    let _ = relay_pin;
    true
}

pub fn group_events_by_kind<T>(events: Vec<T>) -> Vec<Vec<T>> {
    // The verb "group" is fine; the rule targets `group_id`-shaped nouns.
    vec![events]
}
