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
    /// author pubkey → (source event `created_at`, deduped relay list). The
    /// `created_at` is retained even for a cleared (empty) list so a LATER
    /// upsert with an OLDER event is correctly ignored (#3071 keep-newest).
    inner: RwLock<HashMap<String, (u64, Vec<String>)>>,
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
                .map(|(_created_at, relays)| relays)
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

    /// Upsert `pubkey`'s DM-inbox relays from a kind:10050 published at
    /// `created_at`. KEEPS THE NEWEST (#3071): an event OLDER than the cached
    /// one is ignored, so a reused-identity cache that accumulated multiple
    /// kind:10050 across sessions — or a cold-relaunch replay that delivers a
    /// stale event last — never overwrites the current DM-inbox list with a
    /// stale one pointing at a dead relay. (Mirrors the kind:30443 key-package
    /// keep-newest in #3068.) On an equal `created_at` the last write wins
    /// (idempotent for a re-delivered replaceable event).
    ///
    /// An empty `relays` slice is the "author cleared their list" signal; it is
    /// stored as a `created_at`-stamped tombstone (so a subsequent OLDER event
    /// cannot resurrect a stale list) and [`Self::read_relays`] returns `None`.
    ///
    /// D4: the single production writer is [`crate::Kind10050Parser`]; tests may
    /// write directly. D6 — a poisoned lock is logged and dropped.
    pub fn upsert(&self, pubkey: String, created_at: u64, relays: Vec<String>) {
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
        use std::collections::hash_map::Entry;
        match guard.entry(pubkey) {
            Entry::Occupied(mut slot) => {
                if created_at >= slot.get().0 {
                    slot.insert((created_at, relays));
                } else {
                    tracing::debug!(
                        pubkey = %slot.key(),
                        stale_created_at = created_at,
                        kept_created_at = slot.get().0,
                        "DmRelayCache: ignoring stale kind:10050 (kept newer DM-inbox list)"
                    );
                }
            }
            Entry::Vacant(slot) => {
                slot.insert((created_at, relays));
            }
        }
    }

    /// Number of pubkeys with a NON-empty cached relay list (a cleared/tombstone
    /// entry is not counted). Diagnostic + test helper.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .map(|g| g.values().filter(|(_, relays)| !relays.is_empty()).count())
            .unwrap_or(0)
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
            100,
            vec![
                "wss://dm-a.example".to_string(),
                "wss://dm-b.example".to_string(),
            ],
        );

        let resolved = cache
            .read_relays("alice")
            .expect("alice's list is populated");
        assert_eq!(
            resolved,
            vec!["wss://dm-a.example", "wss://dm-b.example"],
            "the upsert payload round-trips unchanged"
        );
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn upsert_with_empty_relays_clears_the_list() {
        let cache = DmRelayCache::new();
        cache.upsert("alice".to_string(), 100, vec!["wss://dm.example".to_string()]);
        assert!(
            cache.read_relays("alice").is_some(),
            "precondition: populated"
        );

        // A NEWER empty kind:10050 (author cleared their list).
        cache.upsert("alice".to_string(), 200, Vec::new());
        assert!(
            cache.read_relays("alice").is_none(),
            "an empty kind:10050 (author cleared their list) resolves to None"
        );
        assert!(cache.is_empty(), "no non-empty list remains");
    }

    #[test]
    fn upsert_replaces_previous_entry_when_newer() {
        let cache = DmRelayCache::new();
        cache.upsert("alice".to_string(), 100, vec!["wss://old.example".to_string()]);
        cache.upsert("alice".to_string(), 200, vec!["wss://new.example".to_string()]);

        let resolved = cache
            .read_relays("alice")
            .expect("alice's list still resolves");
        assert_eq!(
            resolved,
            vec!["wss://new.example".to_string()],
            "the newer kind:10050 must replace the cached list"
        );
        assert_eq!(cache.len(), 1, "only one entry per author");
    }

    /// #3071 — the keep-newest invariant. A STALE kind:10050 arriving AFTER a
    /// fresher one (accumulated across sessions / replayed last on cold
    /// relaunch) must NOT overwrite the current DM-inbox list with a dead relay.
    #[test]
    fn stale_upsert_after_a_newer_one_is_ignored() {
        let cache = DmRelayCache::new();
        // Fresh list (created_at=200) pointing at the LIVE relay arrives first.
        cache.upsert("alice".to_string(), 200, vec!["wss://live.example".to_string()]);
        // A STALE list (created_at=100) pointing at a DEAD relay arrives last.
        cache.upsert("alice".to_string(), 100, vec!["wss://dead.example".to_string()]);

        assert_eq!(
            cache.read_relays("alice"),
            Some(vec!["wss://live.example".to_string()]),
            "the newest kind:10050 (by created_at) must win regardless of ingest order"
        );

        // And a stale CLEAR must not wipe the fresh list either.
        cache.upsert("alice".to_string(), 50, Vec::new());
        assert_eq!(
            cache.read_relays("alice"),
            Some(vec!["wss://live.example".to_string()]),
            "a stale empty kind:10050 must not clear a newer list"
        );
    }

    #[test]
    fn multi_author_seeds_are_independent() {
        let cache = DmRelayCache::new();
        cache.upsert("alice".to_string(), 100, vec!["wss://a.example".to_string()]);
        cache.upsert("bob".to_string(), 100, vec!["wss://b.example".to_string()]);

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
        cache.upsert("alice".to_string(), 100, vec!["wss://via.lookup".to_string()]);

        let as_trait: Arc<dyn DmInboxRelayLookup> = Arc::clone(&cache) as _;
        assert_eq!(
            as_trait.dm_inbox_relays("alice"),
            Some(vec!["wss://via.lookup".to_string()]),
        );
    }
}
