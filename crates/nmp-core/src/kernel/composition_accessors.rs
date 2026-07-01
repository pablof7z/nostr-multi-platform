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

    /// Set the substrate-generic outbound public tags appended to PublicRoutable
    /// publishes (Flow B). Generic `Vec<Vec<String>>` — no NIP-89 noun (D0).
    pub(crate) fn set_outbound_public_tags(&mut self, tags: Vec<Vec<String>>) {
        self.outbound_public_tags = tags;
    }

    /// Read-only accessor for the outbound public tags slot.
    pub(crate) fn outbound_public_tags(&self) -> &[Vec<String>] {
        &self.outbound_public_tags
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

    /// Snapshot 9 typed capability ports from this kernel instance.
    ///
    /// Each field clones the same shared `Arc` the kernel holds internally —
    /// no data is copied. The resulting `KernelPorts` is `Clone + Send` and
    /// may be stored or passed to worker threads.
    ///
    /// **`signer` is absent by design.** The kernel stores signers per-relay;
    /// a typed `SignerPort` field will be added in slice 3 once that shape is
    /// settled. Constructing a stub here would violate the accessor invariant
    /// (the port would look usable but silently fail on every call).
    pub fn ports(&self) -> kernel_ports::KernelPorts {
        kernel_ports::KernelPorts {
            publish: kernel_ports::PublishPort(Arc::clone(&self.publish_store)),
            interest: kernel_ports::InterestPort::new(),
            relay_lifecycle: kernel_ports::RelayLifecyclePort(Arc::clone(&self.outbox_router)),
            protocol_dispatch: kernel_ports::ProtocolDispatchPort(Arc::clone(
                &self.ingest_dispatcher,
            )),
            identity: kernel_ports::IdentityPort(Arc::new(Arc::clone(&self.active_account_handle))),
            reference: kernel_ports::ReferencePort::new(),
            pull_cursor: kernel_ports::PullCursorPort(Arc::clone(&self.pull_cursor_registry)),
            ui: kernel_ports::UiPort(Arc::clone(&self.routing_trace)),
        }
    }
}
