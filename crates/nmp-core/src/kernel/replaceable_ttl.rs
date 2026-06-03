//! Replaceable event freshness configuration (F-TTL).
//!
//! Per the F-TTL design (docs/design/replaceable-freshness.md), this module
//! defines the TTL (time-to-live) policy for replaceable events. The kernel
//! reads this configuration to decide how long a replaceable identity
//! (kind, pubkey, optional d_tag) may remain "fresh" before a re-verification
//! REQ is dispatched.
//!
//! The LMDB sub-db `replaceable_freshness` stores `check_again_after` timestamps
//! keyed by (kind, pubkey, d_tag?); this configuration determines the delta
//! added to `now` to produce the next `check_again_after` value on insertion,
//! replacement, EOSE, or explicit refresh.

use std::collections::BTreeMap;
use std::time::Duration;

/// TTL policy for replaceable events.
///
/// Specifies how long each replaceable kind should be considered "fresh"
/// before re-verification via a fresh REQ is needed.
#[derive(Clone, Debug)]
pub struct ReplaceableTtlConfig {
    /// Per-kind TTL overrides. If a kind is not found here, the `default` is used.
    pub per_kind: BTreeMap<u32, Duration>,
    /// Fallback TTL for kinds not explicitly configured.
    pub default: Duration,
}

impl Default for ReplaceableTtlConfig {
    /// Construct a `ReplaceableTtlConfig` with sensible defaults.
    ///
    /// - kind:0 (user metadata) → 1 hour (D1: apps check profile changes frequently)
    /// - kind:10002 (relay list) → 6 hours (stable; users change relays infrequently)
    /// - all other replaceable kinds → 6 hours
    fn default() -> Self {
        let mut per_kind = BTreeMap::new();
        per_kind.insert(0, Duration::from_secs(3600)); // 1 hour
        per_kind.insert(10002, Duration::from_secs(6 * 3600)); // 6 hours
        Self {
            per_kind,
            default: Duration::from_secs(6 * 3600), // 6 hours
        }
    }
}

impl ReplaceableTtlConfig {
    /// Look up the TTL for a given kind.
    ///
    /// Returns the kind-specific TTL if configured, otherwise the default.
    #[must_use]
    pub fn ttl_for_kind(&self, kind: u32) -> Duration {
        self.per_kind
            .get(&kind)
            .copied()
            .unwrap_or(self.default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = ReplaceableTtlConfig::default();
        assert_eq!(cfg.ttl_for_kind(0), Duration::from_secs(3600));
        assert_eq!(cfg.ttl_for_kind(10002), Duration::from_secs(6 * 3600));
        assert_eq!(cfg.ttl_for_kind(1), Duration::from_secs(6 * 3600)); // Uses default
        assert_eq!(cfg.ttl_for_kind(30023), Duration::from_secs(6 * 3600)); // Uses default
    }

    #[test]
    fn test_custom_config() {
        let mut per_kind = BTreeMap::new();
        per_kind.insert(42, Duration::from_secs(300)); // 5 minutes
        let cfg = ReplaceableTtlConfig {
            per_kind,
            default: Duration::from_secs(1800), // 30 minutes
        };

        assert_eq!(cfg.ttl_for_kind(42), Duration::from_secs(300));
        assert_eq!(cfg.ttl_for_kind(99), Duration::from_secs(1800));
    }
}
