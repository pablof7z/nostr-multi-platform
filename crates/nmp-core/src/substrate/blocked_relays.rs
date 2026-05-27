//! `BlockedRelayLookup` — substrate-generic seam for a per-account "blocked
//! relay" set.
//!
//! The kernel needs to ask "which relay URLs has the active account told us
//! to never publish to / never subscribe through?" when it constructs every
//! `RoutingContext` it hands to the [`crate::substrate::OutboxRouter`].
//! Today the kernel passes an empty [`BlockedRelaySet`] at every call site
//! in `kernel/mailboxes.rs` (see the four `BlockedRelaySet::new()` callers)
//! — wiring this trait through closes that gap.
//!
//! The concrete cache (kind:10006 today, possibly more sources later) lives
//! in the `nmp-router` crate so the kernel never names the wire shape of a
//! kind:10006 event (D0 — `nmp-core` does not embed protocol nouns). The
//! kernel holds an [`Arc<dyn BlockedRelayLookup>`] populated at composition
//! time; a kernel built without any backend uses the
//! [`EmptyBlockedRelayLookup`] default, which returns an empty
//! [`BlockedRelaySet`] for every account (the pre-existing zero-block
//! behaviour, byte-for-byte).
//!
//! ## Why a trait, not a hardwired `HashSet`
//!
//! Mirrors the pattern [`crate::substrate::DmInboxRelayLookup`] uses for
//! kind:10050:
//!
//! - The **writer** is `nmp-router`'s `Kind10006Parser`
//!   ([`crate::substrate::IngestParser`]) — registered with the kernel's
//!   [`crate::substrate::EventIngestDispatcher`] at composition time.
//! - The **reader** is the kernel (`build_routing_context` snapshots a
//!   [`BlockedRelaySet`] off the trait on every router call). The kernel
//!   does NOT know the wire shape of a kind:10006 event.
//!
//! Both ends agree on a shared `Arc<InMemoryBlockedRelayCache>` (the
//! concrete type in `nmp-router`) at composition time; the kernel sees it
//! only as `Arc<dyn BlockedRelayLookup>`.

use std::sync::Arc;

use crate::substrate::routing::BlockedRelaySet;

/// Lookup contract: given a hex pubkey, return the canonicalised blocked
/// relay URL set the active account has declared (kind:10006 in the
/// canonical NIP-51 case, but the trait is wire-shape-agnostic).
///
/// Implementations MUST:
///
/// - Treat "no list known" and "empty list" identically (return an empty
///   [`BlockedRelaySet`] in both cases). The router treats an empty
///   blocked set as "no relays blocked" — a fail-open default the call
///   sites already assume.
/// - Be cheap to call repeatedly — `build_routing_context` calls this once
///   per routing decision, and routing decisions happen on every plan
///   recompile + every publish.
/// - Use interior mutability for any backing store. The trait method takes
///   `&self`; the writer side (the kind:10006 ingest parser) drives
///   mutation through a different method on the concrete type.
pub trait BlockedRelayLookup: Send + Sync {
    /// Resolve `account_pubkey`'s blocked relay set. `account_pubkey` is
    /// lowercase hex. Returns an empty [`BlockedRelaySet`] when no list is
    /// known (the fail-open contract above).
    fn blocked_relays(&self, account_pubkey: &str) -> BlockedRelaySet;
}

/// Default backing — the kernel-cold-start lookup. Always returns an empty
/// [`BlockedRelaySet`], preserving the pre-V-40 byte-for-byte behaviour
/// (the kernel's four `BlockedRelaySet::new()` call sites resolved to an
/// empty set unconditionally).
#[derive(Default)]
pub struct EmptyBlockedRelayLookup;

impl BlockedRelayLookup for EmptyBlockedRelayLookup {
    fn blocked_relays(&self, _account_pubkey: &str) -> BlockedRelaySet {
        BlockedRelaySet::new()
    }
}

/// Convenience: a fresh `Arc<dyn BlockedRelayLookup>` backed by
/// [`EmptyBlockedRelayLookup`] — the kernel's default when no backing
/// cache has been wired in yet.
#[must_use]
pub fn empty_blocked_relay_lookup() -> Arc<dyn BlockedRelayLookup> {
    Arc::new(EmptyBlockedRelayLookup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Stand-in cache that records per-account block lists.
    #[derive(Default)]
    struct TestLookup {
        inner: RwLock<HashMap<String, Vec<String>>>,
    }

    impl TestLookup {
        fn upsert(&self, pubkey: &str, relays: &[&str]) {
            self.inner.write().unwrap().insert(
                pubkey.to_string(),
                relays.iter().map(|s| (*s).to_string()).collect(),
            );
        }
    }

    impl BlockedRelayLookup for TestLookup {
        fn blocked_relays(&self, account_pubkey: &str) -> BlockedRelaySet {
            let mut set = BlockedRelaySet::new();
            if let Some(urls) = self.inner.read().unwrap().get(account_pubkey) {
                for url in urls {
                    set.insert(url.clone());
                }
            }
            set
        }
    }

    #[test]
    fn empty_lookup_returns_empty_set() {
        let lookup: Arc<dyn BlockedRelayLookup> = empty_blocked_relay_lookup();
        assert!(lookup.blocked_relays("alice").is_empty());
    }

    #[test]
    fn populated_lookup_returns_blocked_urls() {
        let lookup = Arc::new(TestLookup::default());
        lookup.upsert("alice", &["wss://bad-a.example", "wss://bad-b.example"]);
        let resolved = lookup.blocked_relays("alice");
        assert!(resolved.contains(&"wss://bad-a.example".to_string()));
        assert!(resolved.contains(&"wss://bad-b.example".to_string()));
    }

    #[test]
    fn unknown_account_returns_empty_set() {
        let lookup = Arc::new(TestLookup::default());
        lookup.upsert("alice", &["wss://blocked.example"]);
        // Bob never declared any blocks — fail-open returns the empty set.
        assert!(lookup.blocked_relays("bob").is_empty());
    }

    #[test]
    fn dyn_trait_object_is_send_sync() {
        // Compile-check: the trait is stored behind `Arc<dyn …>` in the
        // kernel, which requires `Send + Sync`.
        fn assert_send_sync<T: Send + Sync>(_: T) {}
        let lookup: Arc<dyn BlockedRelayLookup> = empty_blocked_relay_lookup();
        assert_send_sync(lookup);
    }
}
