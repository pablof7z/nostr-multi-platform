//! `DmRelayCache` — the NIP-17 kind:10050 DM-inbox relay cache.
//!
//! # Overview
//!
//! NIP-17 § 2 requires every kind:1059 gift-wrap envelope to be published to
//! the **receiver's** kind:10050 DM-relay list — a relay set deliberately
//! distinct from the kind:10002 (NIP-65) generic mailbox. kind:10050 carries
//! `["relay", <url>]` tags (note: the `relay` marker, NOT the `r` marker
//! NIP-65 uses), letting a user route private messages to a privacy-focused
//! relay that is not in their public read set. Collapsing the two would
//! silently leak DM routing onto public relays.
//!
//! `DmRelayCache` is the substrate-owned cache that backs the read side of
//! that contract:
//!
//! * The **writer** is [`crate::Kind10050Parser`] — an
//!   [`nmp_core::substrate::IngestParser`] registered with the kernel's
//!   [`nmp_core::substrate::EventIngestDispatcher`] at composition time.
//! * The **reader** is the kernel
//!   ([`nmp_core::substrate::DmInboxRelayLookup`] impl) — consulted by the
//!   gift-wrap publish path ([`crate::SendGiftWrappedDmCommand`]) and the
//!   planner's `#p`-tagged inbox routing.
//!
//! The same `Arc<DmRelayCache>` is wired on both ends at composition time;
//! the kernel sees it only as `Arc<dyn DmInboxRelayLookup>`.
//!
//! # Empty-list semantics
//!
//! A kind:10050 carrying no `relay` tags is the author's "I cleared my
//! DM-relay list" signal. The cache stores `None` for that pubkey (the
//! `upsert` impl removes any prior entry on an empty input), and
//! [`Self::read_relays`] returns `None` in both the "never published" and
//! "explicitly cleared" cases. The gift-wrap publish path fails closed on
//! `None` — kind:1059 envelopes never fall back to generic Content relays.
//!
//! # D doctrine
//!
//! * **D0** — no kernel noun. The kernel handles only the
//!   `DmInboxRelayLookup` trait shape; the kind:10050 wire format is
//!   confined to this crate.
//! * **D4** — single writer per fact. `Kind10050Parser` is the only
//!   production writer; tests use the trait directly. Interior mutability is
//!   a `RwLock<HashMap<…>>` so the parser's `&self` method can write.
//! * **D6** — a poisoned lock is a no-op rather than a panic. The cache
//!   methods log the error to `tracing` and degrade to the empty case.

use std::collections::HashMap;
use std::sync::RwLock;

use nmp_core::substrate::DmInboxRelayLookup;

/// In-memory NIP-17 DM-relay cache (kind:10050).
///
/// One entry per author pubkey, valued by the deduped, canonicalised
/// DM-inbox relay URL list. Cheap to clone behind an `Arc` — every
/// internal field is `Default`-constructed empty and grows only as
/// kind:10050 events arrive.
///
/// Wrapped in `Arc` at composition time so the same handle is the
/// writer (consumed by `Kind10050Parser`) and the reader (consumed by
/// the kernel as `Arc<dyn DmInboxRelayLookup>`).
#[derive(Default)]
pub struct DmRelayCache {
    inner: RwLock<HashMap<String, Vec<String>>>,
}

impl DmRelayCache {
    /// Construct an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve `pubkey`'s DM-inbox relays. Returns `None` when no list is
    /// known (never published OR explicitly cleared via an empty
    /// kind:10050). The fail-closed contract NIP-17 § 2 requires.
    ///
    /// D6 — a poisoned lock degrades to `None` rather than panicking.
    #[must_use]
    pub fn read_relays(&self, pubkey: &str) -> Option<Vec<String>> {
        match self.inner.read() {
            Ok(guard) => guard
                .get(pubkey)
                .filter(|relays| !relays.is_empty())
                .cloned(),
            Err(e) => {
                tracing::warn!(
                    pubkey = %pubkey,
                    error = ?e,
                    "DmRelayCache read lock poisoned — degrading to None (D6)"
                );
                None
            }
        }
    }

    /// Upsert `pubkey`'s DM-inbox relays. An empty `relays` slice removes
    /// the entry (the "author cleared their list" path the kind:10050
    /// supersession contract requires).
    ///
    /// D4: the single production writer is [`crate::Kind10050Parser`];
    /// tests may write directly through this method. D6 — a poisoned
    /// lock is logged and dropped (no panic, no partial write).
    pub fn upsert(&self, pubkey: String, relays: Vec<String>) {
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(
                    pubkey = %pubkey,
                    error = ?e,
                    "DmRelayCache write lock poisoned — dropping upsert (D6)"
                );
                return;
            }
        };
        if relays.is_empty() {
            guard.remove(&pubkey);
        } else {
            guard.insert(pubkey, relays);
        }
    }

    /// Number of pubkeys with cached entries. Diagnostic + test helper.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
    }

    /// `true` iff no pubkey is cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl DmInboxRelayLookup for DmRelayCache {
    fn dm_inbox_relays(&self, pubkey: &str) -> Option<Vec<String>> {
        self.read_relays(pubkey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn cold_cache_returns_none() {
        let cache = DmRelayCache::new();
        assert!(cache.read_relays("alice").is_none());
        assert!(cache.is_empty());
    }

    #[test]
    fn upsert_then_read_round_trips() {
        let cache = DmRelayCache::new();
        cache.upsert(
            "alice".to_string(),
            vec!["wss://dm-a.example".to_string(), "wss://dm-b.example".to_string()],
        );

        let resolved = cache.read_relays("alice").expect("alice's list is populated");
        assert_eq!(
            resolved,
            vec!["wss://dm-a.example", "wss://dm-b.example"],
            "the upsert payload round-trips unchanged"
        );
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn upsert_with_empty_relays_removes_entry() {
        let cache = DmRelayCache::new();
        cache.upsert("alice".to_string(), vec!["wss://dm.example".to_string()]);
        assert!(cache.read_relays("alice").is_some(), "precondition: populated");

        cache.upsert("alice".to_string(), Vec::new());
        assert!(
            cache.read_relays("alice").is_none(),
            "an empty kind:10050 (author cleared their list) removes the entry"
        );
        assert!(cache.is_empty(), "the entry must be gone, not stored empty");
    }

    #[test]
    fn upsert_replaces_previous_entry() {
        let cache = DmRelayCache::new();
        cache.upsert("alice".to_string(), vec!["wss://old.example".to_string()]);
        cache.upsert("alice".to_string(), vec!["wss://new.example".to_string()]);

        let resolved = cache.read_relays("alice").expect("alice's list still resolves");
        assert_eq!(
            resolved,
            vec!["wss://new.example".to_string()],
            "the newer kind:10050 must replace the cached list"
        );
        assert_eq!(cache.len(), 1, "only one entry per author");
    }

    #[test]
    fn multi_author_seeds_are_independent() {
        let cache = DmRelayCache::new();
        cache.upsert("alice".to_string(), vec!["wss://a.example".to_string()]);
        cache.upsert("bob".to_string(), vec!["wss://b.example".to_string()]);

        assert_eq!(
            cache.read_relays("alice"),
            Some(vec!["wss://a.example".to_string()]),
        );
        assert_eq!(
            cache.read_relays("bob"),
            Some(vec!["wss://b.example".to_string()]),
        );
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn satisfies_dm_inbox_relay_lookup_trait_via_arc_dyn() {
        // Compile + behaviour check: the kernel holds this cache behind
        // `Arc<dyn DmInboxRelayLookup>`. Both the trait method and the
        // inherent method MUST return the same payload.
        let cache = Arc::new(DmRelayCache::new());
        cache.upsert("alice".to_string(), vec!["wss://via.lookup".to_string()]);

        let as_trait: Arc<dyn DmInboxRelayLookup> = Arc::clone(&cache) as _;
        assert_eq!(
            as_trait.dm_inbox_relays("alice"),
            Some(vec!["wss://via.lookup".to_string()]),
        );
    }
}
