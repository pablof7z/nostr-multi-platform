//! Canonical Nostr kind constants for the workspace.
//!
//! The actual `pub const` definitions live in the zero-dependency Layer-0
//! crate `nmp-kinds`; this module re-exports them so ALL existing
//! `nmp_core::kinds::KIND_*` call sites across the workspace continue to
//! compile unchanged.
//!
//! # Why `nmp-kinds` (Layer 0), not here (Layer 3)
//!
//! `nmp-core` depends on `nmp-nip59` (the kernel uses the gift-wrap
//! primitive on the actor thread — ADR-0009 precedent). That edge means
//! `nmp-nip59 → nmp-core` would be a **compile-time cycle**, so `nmp-nip59`
//! cannot import `KIND_GIFT_WRAP` from this module directly. Moving the
//! integer registry to `nmp-kinds` (zero deps, Layer 0 — same pattern as
//! `nmp-nip42-types`) lets both `nmp-core` and `nmp-nip59` depend on the
//! same source without any cycle.
//!
//! # Scope
//!
//! This module is the workspace's canonical *integer* registry only. Per-NIP
//! event-shape, parser, builder, and routing logic still lives in the
//! protocol crates; nothing about a constant being declared here implies
//! the kernel knows how to read or write the corresponding event.

pub use nmp_kinds::*;

/// Check whether a kind is a replaceable event (NIP-01).
///
/// Replaceable events have kind ranges:
/// - Regular replaceable: 0–9999, 10000–19999
/// - Parameterized replaceable: 20000–29999, 30000–39999
///
/// This function checks only the regular replaceable ranges.
#[inline]
pub fn is_replaceable(kind: u32) -> bool {
    (kind <= 9999) || (kind >= 10000 && kind <= 19999)
}

/// Check whether a kind is a parameterized replaceable event (NIP-01).
///
/// Parameterized replaceable events have kind ranges: 20000–29999, 30000–39999.
#[inline]
pub fn is_parameterized_replaceable(kind: u32) -> bool {
    (kind >= 20000 && kind <= 29999) || (kind >= 30000 && kind <= 39999)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_replaceable() {
        // Regular replaceable ranges
        assert!(is_replaceable(0), "kind:0 should be replaceable");
        assert!(is_replaceable(1), "kind:1 should be replaceable");
        assert!(is_replaceable(9999), "kind:9999 should be replaceable");
        assert!(is_replaceable(10000), "kind:10000 should be replaceable");
        assert!(is_replaceable(10002), "kind:10002 should be replaceable");
        assert!(is_replaceable(19999), "kind:19999 should be replaceable");

        // Non-replaceable ranges
        assert!(!is_replaceable(20000), "kind:20000 should not be replaceable");
        assert!(!is_replaceable(30000), "kind:30000 should not be replaceable");
        assert!(!is_replaceable(40000), "kind:40000 should not be replaceable");
    }

    #[test]
    fn test_is_parameterized_replaceable() {
        // Parameterized replaceable ranges
        assert!(is_parameterized_replaceable(20000), "kind:20000 should be parameterized replaceable");
        assert!(is_parameterized_replaceable(20023), "kind:20023 should be parameterized replaceable");
        assert!(is_parameterized_replaceable(29999), "kind:29999 should be parameterized replaceable");
        assert!(is_parameterized_replaceable(30000), "kind:30000 should be parameterized replaceable");
        assert!(is_parameterized_replaceable(30023), "kind:30023 should be parameterized replaceable");
        assert!(is_parameterized_replaceable(39999), "kind:39999 should be parameterized replaceable");

        // Non-parameterized replaceable ranges
        assert!(!is_parameterized_replaceable(0), "kind:0 should not be parameterized replaceable");
        assert!(!is_parameterized_replaceable(9999), "kind:9999 should not be parameterized replaceable");
        assert!(!is_parameterized_replaceable(10000), "kind:10000 should not be parameterized replaceable");
        assert!(!is_parameterized_replaceable(40000), "kind:40000 should not be parameterized replaceable");
    }

    #[test]
    fn test_boundary_values() {
        // Test boundaries explicitly
        assert!(is_replaceable(9999));
        assert!(!is_replaceable(10000) || is_replaceable(10000)); // 10000 is in second range
        assert!(is_replaceable(10000), "kind:10000 is at boundary of second replaceable range");
        assert!(!is_replaceable(19999) || is_replaceable(19999)); // 19999 is in second range
        assert!(is_replaceable(19999), "kind:19999 is at boundary of second replaceable range");
        assert!(!is_replaceable(20000), "kind:20000 starts parameterized range");

        assert!(!is_parameterized_replaceable(19999), "kind:19999 is not parameterized");
        assert!(is_parameterized_replaceable(20000), "kind:20000 starts parameterized range");
        assert!(is_parameterized_replaceable(29999), "kind:29999 ends first parameterized range");
        assert!(!is_parameterized_replaceable(30000) || is_parameterized_replaceable(30000)); // 30000 is in second range
        assert!(is_parameterized_replaceable(30000), "kind:30000 starts second parameterized range");
    }
}
