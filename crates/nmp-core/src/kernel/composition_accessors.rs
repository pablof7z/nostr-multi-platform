//! Kernel composition-seam accessors.

use super::*;

impl Kernel {
    /// Inject the DM-inbox relay lookup (V-40 composition seam). Production
    /// composition installs the shared cache so the recipient reader and the
    /// planner-side adapter see the same kind:10050 entries the parser writes.
    /// MUST be called before the first kind:10050 event is ingested.
    pub(crate) fn set_dm_inbox_relay_lookup(&mut self, lookup: Arc<dyn DmInboxRelayLookup>) {
        self.dm_inbox_relays = lookup;
    }

    /// Inject the kind:0 profile lookup (composition seam, ADR-0057 PR 2).
    /// The cache is an independent store, not a write-through pair, so late
    /// swaps would lose already-ingested entries.
    pub(crate) fn set_profile_lookup(&mut self, lookup: Arc<dyn ProfileLookup>) {
        self.profile_lookup = lookup;
    }

    /// Read accessor for the injected profile lookup.
    pub(crate) fn profile_lookup(&self) -> &dyn ProfileLookup {
        &*self.profile_lookup
    }

    /// Inject the kind:3 contacts lookup (composition seam, ADR-0057 PR 3).
    pub(crate) fn set_contacts_lookup(&mut self, lookup: Arc<dyn ContactsLookup>) {
        self.contacts_lookup = lookup;
    }

    /// Read accessor for the injected contacts lookup.
    pub(crate) fn contacts_lookup(&self) -> &dyn ContactsLookup {
        &*self.contacts_lookup
    }

    /// Inject the blocked-relay lookup used by both routing and publish.
    pub(crate) fn set_blocked_relay_lookup(&mut self, lookup: Arc<dyn BlockedRelayLookup>) {
        self.publish_engine
            .set_blocked_relay_lookup(Arc::clone(&lookup));
        self.blocked_relays = lookup;
    }

    /// Shared handle used by `kernel/mailboxes.rs::build_routing_context`.
    pub(crate) fn blocked_relays_arc(&self) -> Arc<dyn BlockedRelayLookup> {
        Arc::clone(&self.blocked_relays)
    }

    /// Override the active-account bootstrap Tailing self-kinds list.
    pub(crate) fn set_bootstrap_self_kinds_override(&mut self, kinds: Option<Vec<u32>>) {
        self.bootstrap_self_kinds_override = kinds;
    }

    /// Read-only accessor for the bootstrap self-kinds override slot.
    pub(crate) fn bootstrap_self_kinds_override(&self) -> Option<&[u32]> {
        self.bootstrap_self_kinds_override.as_deref()
    }

    /// Replace the kernel's shared ingest-dispatcher slot.
    pub(crate) fn set_ingest_dispatcher_slot(
        &mut self,
        slot: Arc<std::sync::RwLock<EventIngestDispatcher>>,
    ) {
        self.ingest_dispatcher = slot;
    }

    /// Shared handle to the injected `Arc<dyn DmInboxRelayLookup>`.
    pub(crate) fn dm_inbox_relays_arc(&self) -> Arc<dyn DmInboxRelayLookup> {
        Arc::clone(&self.dm_inbox_relays)
    }

    /// Register an ingest parser against the shared dispatcher slot.
    #[allow(dead_code)] // Wired through `NmpApp` at composition time.
    pub(crate) fn register_ingest_parser(
        &self,
        kind: u32,
        parser: Arc<dyn crate::substrate::IngestParser>,
    ) {
        if let Ok(mut d) = self.ingest_dispatcher.write() {
            d.register_kind(kind, parser);
        }
    }

    /// Shared handle to the kernel's ingest-dispatcher slot.
    pub(crate) fn ingest_dispatcher_slot(&self) -> Arc<std::sync::RwLock<EventIngestDispatcher>> {
        Arc::clone(&self.ingest_dispatcher)
    }

    /// Drain pending backoff hints enqueued during the last `handle_message`.
    pub(crate) fn take_backoff_hints(&mut self) -> Vec<(String, BackoffHint)> {
        std::mem::take(&mut self.pending_backoff_hints)
    }
}
