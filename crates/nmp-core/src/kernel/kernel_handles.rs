//! Shared-slot handle accessors + prepopulate / seed / mailbox helpers.
//!
//! Extracted from `kernel/mod.rs` (`impl Kernel`) to honour the 500-LOC ceiling.

use super::*;

impl Kernel {
    /// Borrow the kernel's `EventStore` handle.
    #[must_use]
    pub fn event_store_handle(&self) -> Arc<dyn EventStore> {
        Arc::clone(&self.store)
    }

    /// Borrow the kernel's pull-cursor registry handle (ADR-0058 §3, step 3b).
    #[must_use]
    pub fn pull_cursor_registry_handle(&self) -> pull_cursor::PullCursorRegistrySlot {
        Arc::clone(&self.pull_cursor_registry)
    }

    /// Borrow the kernel's indexer-relays slot.
    #[must_use]
    pub fn indexer_relays_handle(&self) -> IndexerRelaysSlot {
        Arc::clone(&self.indexer_relays_handle)
    }

    /// Borrow the kernel's local-write-relays slot.
    #[must_use]
    pub fn local_write_relays_handle(&self) -> LocalWriteRelaysSlot {
        Arc::clone(&self.local_write_relays_handle)
    }

    /// Borrow the kernel's active-account-pubkey slot.
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        Arc::clone(&self.active_account_handle)
    }

    /// Read the current active-account pubkey (lowercase hex), or `None`.
    #[must_use]
    pub(crate) fn active_account_pubkey(&self) -> Option<&str> {
        self.active_account.as_deref()
    }

    /// V-51 — borrow the kernel's routing-trace projection.
    #[must_use]
    pub fn routing_trace(&self) -> Arc<routing_trace::RoutingTraceProjection> {
        Arc::clone(&self.routing_trace)
    }

    /// Pre-populate the NIP-65 mailbox cache from a just-signed kind:10002 event.
    pub(crate) fn prepopulate_author_relay_list(
        &mut self,
        pubkey: String,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) {
        let parsed = parse_relay_list_to_substrate(&tags);
        let empty = parsed.read.is_empty() && parsed.write.is_empty() && parsed.both.is_empty();
        if empty {
            self.mailbox_cache.remove(&pubkey);
        } else {
            self.mailbox_cache.upsert(pubkey.clone(), parsed);
        }
        self.lifecycle
            .enqueue_trigger(CompileTrigger::Nip65Arrived { pubkey, created_at });
    }

    /// Read-only access to the substrate `MailboxCache`.
    pub(crate) fn mailbox_cache(&self) -> &dyn MailboxCache {
        &*self.mailbox_cache
    }

    /// Test-only: push a NIP-65 cache entry without going through the kind:10002 ingest path.
    #[cfg(test)]
    pub(crate) fn seed_mailbox_relay_list(
        &self,
        pubkey: &str,
        read: Vec<String>,
        write: Vec<String>,
        both: Vec<String>,
    ) {
        self.mailbox_cache
            .upsert(pubkey.to_string(), ParsedRelayList { read, write, both });
    }

    /// Test-only: shared handle to the substrate `MailboxCache`.
    #[cfg(test)]
    pub(crate) fn mailbox_cache_arc(&self) -> Arc<dyn MailboxCache> {
        Arc::clone(&self.mailbox_cache)
    }

    /// Record a store-open failure reason (degraded-mode diagnostic) AFTER kernel
    /// construction.
    ///
    /// Native LMDB threads the reason in at construction (`build_event_store`).
    /// Browser composition opens the durable OPFS-SQLite store **asynchronously
    /// before the kernel exists** (ADR-0054 §1), so when that open fails the
    /// reason is recorded here post-hoc and surfaces through the **same** Tier-3
    /// `store_open_failure` snapshot channel native uses (#1007 PR-8). Idempotent
    /// last-writer-wins; D6 — no stderr, no panic.
    pub(crate) fn set_store_open_failure(&mut self, reason: impl Into<String>) {
        self.store_open_failure = Some(reason.into());
    }

    /// Read the recorded store-open failure reason, if any (degraded-mode
    /// diagnostic). `None` for a healthy open (#1007 PR-8). Test/test-support
    /// only: production reads the reason through the Tier-3 snapshot, never this
    /// accessor (its sole consumer is `KernelReducer::store_open_failure`).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn store_open_failure(&self) -> Option<&str> {
        self.store_open_failure.as_deref()
    }

    /// Test-only: inject a `store_open_failure` string without requiring a real LMDB failure.
    #[cfg(test)]
    pub(crate) fn set_store_open_failure_for_test(&mut self, reason: impl Into<String>) {
        self.set_store_open_failure(reason);
    }

    /// Test-only: set `active_account` directly for diagnostic path tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_active_account_for_test(&mut self, pubkey: impl Into<String>) {
        self.active_account = Some(pubkey.into());
    }

    /// Test-only: seed a profile view through the core lookup seam.
    #[cfg(test)]
    pub(crate) fn seed_profile_view_for_test(
        &mut self,
        pubkey: &str,
        view: crate::substrate::ProfileView,
    ) {
        let before = self.profile_lookup().profile(pubkey);
        self.test_profile_lookup.seed_view(pubkey, view);
        let after = self.profile_lookup().profile(pubkey);
        if before != after {
            self.cached_estimated_store_bytes.set(None);
            self.projection_rev_tracker.source_versions.bump_profiles();
            if self.profile_claims.contains_key(pubkey) {
                self.projection_rev_tracker
                    .source_versions
                    .bump_profile_row(pubkey);
            }
        }
    }

    /// Read-only access to the injected `OutboxRouter`.
    #[allow(dead_code)] // Reserved for follow-on wiring of actual routing call sites.
    pub(crate) fn outbox_router(&self) -> &dyn OutboxRouter {
        &*self.outbox_router
    }
}
