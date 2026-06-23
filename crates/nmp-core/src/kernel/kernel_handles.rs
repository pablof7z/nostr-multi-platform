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

    /// Pre-populate the contacts cache at sign-in without fabricating a kind:3 event.
    pub(crate) fn prepopulate_contacts(&mut self, pubkey: String, follows: Vec<String>) {
        let created_at = self.now_secs();
        // (1) Write the contacts cache directly — non-ingest writer seam, NO
        // fabricated event through the dispatcher / observer fan-out.
        self.contacts_lookup().upsert(
            pubkey.clone(),
            crate::substrate::ContactsView {
                // Maximal sentinel id: a real signed kind:3 (a 64-hex id, always
                // `< "f"*64`) supersedes this seed on a `created_at` tie.
                event_id: "f".repeat(64),
                created_at,
                follows: follows.clone(),
            },
        );
        self.cached_estimated_store_bytes.set(None);
        // (2) Drive the kernel-owned follow-feed effects directly (active-account
        // scoped, like the chokepoint transition that calls the same body) —
        // WITHOUT `notify_event_observers`.
        if self.active_account.as_deref() == Some(pubkey.as_str()) {
            self.on_active_contacts_changed(&pubkey, follows, created_at);
        }
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
    #[cfg(any(test, feature = "test-support"))]
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
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn mailbox_cache_arc(&self) -> Arc<dyn MailboxCache> {
        Arc::clone(&self.mailbox_cache)
    }

    /// Test-only: inject a `store_open_failure` string without requiring a real LMDB failure.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_store_open_failure_for_test(&mut self, reason: impl Into<String>) {
        self.store_open_failure = Some(reason.into());
    }

    /// Test-only: set `active_account` directly for diagnostic path tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_active_account_for_test(&mut self, pubkey: impl Into<String>) {
        self.active_account = Some(pubkey.into());
    }

    /// Test-only: cache a kind:0 profile without going through the ingest chokepoint.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn seed_profile_kind0_for_test(
        &self,
        pubkey: &str,
        event_id: &str,
        created_at: u64,
        content: &str,
    ) -> bool {
        self.test_profile_cache
            .ingest_kind0(pubkey, event_id, created_at, content)
    }

    /// Read-only access to the injected `OutboxRouter`.
    #[allow(dead_code)] // Reserved for follow-on wiring of actual routing call sites.
    pub(crate) fn outbox_router(&self) -> &dyn OutboxRouter {
        &*self.outbox_router
    }
}
