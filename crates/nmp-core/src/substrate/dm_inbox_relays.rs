//! `DmInboxRelayLookup` — substrate-generic seam for a per-pubkey "DM-inbox
//! relay" cache.
//!
//! The kernel needs to ask "for receiver `P`, what are P's DM-inbox relays?"
//! when it routes gift-wrap envelopes and when the planner-side
//! [`crate::planner::compiler::mailbox::MailboxCache`] adapter resolves
//! `#p` filters under [`crate::planner::PTagRouting::Nip17DmRelays`]. The
//! concrete cache (NIP-17 kind:10050) lives in the `nmp-nip17` crate so the
//! kernel never names the NIP-17 noun (D0). The kernel holds an
//! [`Arc<dyn DmInboxRelayLookup>`] populated at composition time; a kernel
//! built without any DM-relay backend uses the [`EmptyDmInboxRelayLookup`]
//! default, which returns `None` for every pubkey (the fail-closed semantics
//! the gift-wrap publish path already expects).
//!
//! ## Why this trait, not a hardwired `HashMap`
//!
//! Before V-40 the kernel carried a bespoke `dm_relay_lists:
//! HashMap<String, Vec<String>>` field and a hardwired kind:10050 ingest arm.
//! Those are NIP-17 nouns inside `nmp-core`. The trait + slot replaces both:
//!
//! - The **writer** is `nmp-nip17`'s `Kind10050Parser`
//!   ([`crate::substrate::IngestParser`]) — registered with the kernel's
//!   [`crate::substrate::EventIngestDispatcher`] at composition time.
//! - The **reader** is the kernel (DM-inbox planner adapter +
//!   `recipient_dm_relays` helper) — it consults this trait through the
//!   substrate-generic shape. The kernel does NOT know the wire shape of a
//!   kind:10050 event.
//!
//! Both ends agree on a shared `Arc<DmRelayCache>` (the concrete type in
//! `nmp-nip17`) at composition time; the kernel sees it only as
//! `Arc<dyn DmInboxRelayLookup>`.

use std::sync::Arc;

/// Lookup contract: given a hex pubkey, return the canonicalised DM-inbox
/// relay URLs the receiver declared (or `None` when no list is known).
///
/// Implementations MUST:
///
/// - Treat an *empty* list (a receiver who explicitly cleared their list) as
///   `None`. The downstream gift-wrap publish path fails closed on `None`;
///   collapsing the two cases keeps that fail-closed contract structural.
/// - Be cheap to call repeatedly — the planner-side adapter calls this once
///   per `#p`-tagged pubkey on every plan recompile.
/// - Use interior mutability for any backing store. The trait method takes
///   `&self`; the writer side (the kind:10050 ingest parser) drives mutation
///   through a different method on the concrete type.
pub trait DmInboxRelayLookup: Send + Sync {
    /// Resolve `pubkey`'s DM-inbox relays. `pubkey` is lowercase hex.
    fn dm_inbox_relays(&self, pubkey: &str) -> Option<Vec<String>>;
}

/// Default backing — the kernel-cold-start lookup. Always returns `None`,
/// causing the gift-wrap send path to fail closed (the contract NIP-17 § 2
/// requires for a kind:1059 envelope whose receiver has no kind:10050).
#[derive(Default)]
pub struct EmptyDmInboxRelayLookup;

impl DmInboxRelayLookup for EmptyDmInboxRelayLookup {
    fn dm_inbox_relays(&self, _pubkey: &str) -> Option<Vec<String>> {
        None
    }
}

/// Convenience: a fresh `Arc<dyn DmInboxRelayLookup>` backed by
/// [`EmptyDmInboxRelayLookup`] — the kernel's default when no NIP-17 cache
/// has been wired in yet.
#[must_use]
pub fn empty_dm_inbox_relay_lookup() -> Arc<dyn DmInboxRelayLookup> {
    Arc::new(EmptyDmInboxRelayLookup)
}

/// Test-only in-memory cache for the substrate `DmInboxRelayLookup` trait.
///
/// Production composition uses `nmp_nip17::DmRelayCache`. This stand-in
/// lives inside `nmp-core` so the crate's own tests (which cannot depend on
/// `nmp-nip17`) can still exercise the gift-wrap publish path and the
/// kernel's `recipient_dm_relays` reader end-to-end. Mirrors the production
/// cache's shape (`RwLock<HashMap<pubkey, Vec<relay>>>`) so a future
/// abstraction over both impls collapses cleanly.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct TestDmInboxRelayCache {
    inner: std::sync::RwLock<std::collections::HashMap<String, Vec<String>>>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestDmInboxRelayCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed `pubkey`'s DM-inbox relays. An empty `relays` slice removes the
    /// entry (matches the trait's "empty list collapses to None" contract).
    /// URLs are canonicalised (lowercase scheme+host, strip empty-path
    /// trailing slash) so tests that drive the production
    /// `Kind10050Parser` and tests that seed through this helper produce
    /// identical cache shapes.
    pub fn upsert(&self, pubkey: &str, relays: &[&str]) {
        // D15 — degrade-gracefully on poisoned lock; silently drop the
        // mutation rather than panic on the actor thread. Mirrors the
        // policy documented in `nmp_router::cache`.
        let Ok(mut guard) = self.inner.write() else {
            return;
        };
        if relays.is_empty() {
            guard.remove(pubkey);
        } else {
            guard.insert(
                pubkey.to_string(),
                relays.iter().map(|s| canonicalize_test_relay(s)).collect(),
            );
        }
    }
}

/// Canonicalise `url` the same way [`crate::CanonicalRelayUrl`] does
/// (lowercase scheme+host, strip the empty-path trailing slash). Kept
/// inline rather than reaching for the kernel's helper because
/// `dm_inbox_relays` lives in `substrate/`, which must not depend on
/// `crate::relay::*`.
#[cfg(any(test, feature = "test-support"))]
fn canonicalize_test_relay(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("wss://") {
        let (host_port, path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, ""),
        };
        let canonical_host = host_port.to_lowercase();
        let canonical_path = if path == "/" { "" } else { path };
        return format!("wss://{canonical_host}{canonical_path}");
    }
    url.to_string()
}

#[cfg(any(test, feature = "test-support"))]
impl DmInboxRelayLookup for TestDmInboxRelayCache {
    fn dm_inbox_relays(&self, pubkey: &str) -> Option<Vec<String>> {
        // D15 — degrade-gracefully on poisoned lock; treat as "no list known"
        // (which the gift-wrap publish path translates to fail-closed per
        // NIP-17 § 2, matching the production cold-start contract).
        self.inner
            .read()
            .ok()
            .and_then(|g| g.get(pubkey).filter(|r| !r.is_empty()).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Stand-in cache that records what the kernel queries. Mirrors the
    /// concrete `nmp-nip17::DmRelayCache` shape (an `RwLock<HashMap<…>>` over
    /// `pubkey → relays`) without taking that crate as a dev-dep — the
    /// substrate trait is the contract we test here.
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

    impl DmInboxRelayLookup for TestLookup {
        fn dm_inbox_relays(&self, pubkey: &str) -> Option<Vec<String>> {
            self.inner
                .read()
                .unwrap()
                .get(pubkey)
                .filter(|relays| !relays.is_empty())
                .cloned()
        }
    }

    #[test]
    fn empty_lookup_returns_none() {
        let lookup: Arc<dyn DmInboxRelayLookup> = empty_dm_inbox_relay_lookup();
        assert!(lookup.dm_inbox_relays("alice").is_none());
    }

    #[test]
    fn populated_lookup_returns_relays() {
        let lookup = Arc::new(TestLookup::default());
        lookup.upsert("alice", &["wss://dm-a.example", "wss://dm-b.example"]);
        let resolved = lookup
            .dm_inbox_relays("alice")
            .expect("alice's list is populated");
        assert_eq!(resolved, vec!["wss://dm-a.example", "wss://dm-b.example"]);
    }

    #[test]
    fn empty_relay_list_collapses_to_none() {
        // The trait contract: an empty list (author cleared their list) is
        // indistinguishable from "no list" — both return `None`. This keeps
        // the gift-wrap publish path's fail-closed gate structural.
        let lookup = Arc::new(TestLookup::default());
        lookup.upsert("alice", &[]);
        assert!(lookup.dm_inbox_relays("alice").is_none());
    }

    #[test]
    fn dyn_trait_object_is_send_sync() {
        // Compile-check: the trait is stored behind `Arc<dyn …>` in the
        // kernel, which requires `Send + Sync`.
        fn assert_send_sync<T: Send + Sync>(_: T) {}
        let lookup: Arc<dyn DmInboxRelayLookup> = empty_dm_inbox_relay_lookup();
        assert_send_sync(lookup);
    }
}
