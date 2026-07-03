//! Read-once substrate/cache/factory configuration setters for `NmpApp`.

use crate::{NmpApp, NmpConfigStatus};

impl NmpApp {
    // ADR-0072 §D3 — per-app signer-port accessors live in the cohesive
    // `signer_ports` sibling module (another `impl NmpApp` block).

    /// V-40 — register a [`nmp_core::substrate::IngestParser`] for `kind`
    /// against the shared `EventIngestDispatcher` slot. The same `Arc`
    /// the actor binds onto the kernel, so a registration is visible to
    /// the ingest path immediately (no actor round-trip needed).
    ///
    /// Per-NIP crates call this through their crate-level installer (for
    /// example, `nmp_nip17::register(...)` installs the `Kind10050Parser`).
    /// MUST be called before `nmp_app_start` so the kernel sees the parser when
    /// the first event of `kind` arrives.
    ///
    /// D6 — a poisoned dispatcher lock is a silent no-op (the
    /// registration is dropped; existing registrations are preserved).
    pub fn register_ingest_parser(
        &self,
        kind: u32,
        parser: std::sync::Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "ingest_parser",
            format!("kind:{kind}"),
            std::any::type_name::<dyn nmp_core::substrate::IngestParser>(),
        ) {
            return status;
        }
        if let Ok(mut d) = self.capability_ports.ingest_dispatcher_slot.write() {
            // ADR-0069 Part 2 — record the parser registration. This is an
            // additive seam (multiple parsers per kind coexist), so a pre-start
            // call is always `Installed`. Post-start calls return
            // `AlreadyStarted` above and record `DroppedLateWiring`.
            self.composition_ledger.record(
                "ingest_parser",
                format!("kind:{kind}"),
                std::any::type_name::<dyn nmp_core::substrate::IngestParser>(),
                nmp_core::Disposition::Installed,
                None,
            );
            d.register_kind(kind, parser);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Slot-keyed replace: evict the prior parser registered under `slot_key`
    /// for `kind` (if any), install `parser` under the same slot, and return
    /// the previous parser for that slot. Parsers registered under other slot
    /// keys are untouched.
    ///
    /// Used by lifecycle-managed singleton parsers (e.g. the NIP-17 DM inbox
    /// parser — swapped to a fresh projection instance on account switch so
    /// accumulated in-memory messages are cleared). D6 — a poisoned dispatcher
    /// lock is a silent no-op returning `None`.
    pub fn replace_ingest_parser(
        &self,
        kind: u32,
        slot_key: &'static str,
        parser: std::sync::Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<std::sync::Arc<dyn nmp_core::substrate::IngestParser>> {
        if let Ok(mut d) = self.capability_ports.ingest_dispatcher_slot.write() {
            d.replace_kind_parser(kind, slot_key, parser)
        } else {
            None
        }
    }

    // #1811 — `register_search_scope` (FTS scope registration) lives in the
    // cohesive `app_config_search` sibling module (another `impl NmpApp` block)
    // to keep this file under the file-size hard cap.

    /// Remove the parser registered under `slot_key` for `kind`, if any.
    /// Used by teardown paths (e.g. Marmot sign-out) to clear a
    /// lifecycle-managed slot without installing a replacement.
    /// D6 — a poisoned dispatcher lock is a silent no-op.
    pub fn unregister_ingest_parser(&self, kind: u32, slot_key: &'static str) {
        if let Ok(mut d) = self.capability_ports.ingest_dispatcher_slot.write() {
            d.remove_kind_parser_slot(kind, slot_key);
        }
    }

    /// Slot-keyed replace for a kind range.  Mirrors
    /// [`Self::replace_ingest_parser`] but registers against a `Range<u32>`
    /// instead of a single kind — used by all-kinds parsers (e.g. the
    /// chirp-tui debug raw-event cache). D6 — a poisoned dispatcher lock is a
    /// silent no-op returning `None`.
    pub fn replace_ingest_parser_range(
        &self,
        range: std::ops::Range<u32>,
        slot_key: &'static str,
        parser: std::sync::Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<std::sync::Arc<dyn nmp_core::substrate::IngestParser>> {
        if let Ok(mut d) = self.capability_ports.ingest_dispatcher_slot.write() {
            d.replace_range_parser(range, slot_key, parser)
        } else {
            None
        }
    }

    /// Remove the range-parser registered under `slot_key`, if any.
    /// D6 — a poisoned dispatcher lock is a silent no-op.
    pub fn unregister_ingest_parser_range(&self, slot_key: &'static str) {
        if let Ok(mut d) = self.capability_ports.ingest_dispatcher_slot.write() {
            d.remove_range_parser_slot(slot_key);
        }
    }

    /// V-40 — install the kernel's [`nmp_core::substrate::DmInboxRelayLookup`]
    /// handle. The NIP-17 crate-level installer hands in a concrete
    /// `DmRelayCache`; the same `Arc` is the writer side fed by the kind:10050
    /// parser registered above + the reader side the kernel exposes through
    /// `recipient_dm_relays` and the planner-side `KernelMailboxes` adapter.
    ///
    /// MUST be called before `nmp_app_start` AND before any kind:10050
    /// event is ingested (the caches are independent stores; a late swap
    /// would lose entries written into the old cache).
    pub fn set_dm_inbox_relay_lookup(
        &self,
        lookup: std::sync::Arc<dyn nmp_core::substrate::DmInboxRelayLookup>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "dm_inbox_relay_lookup",
            "dm_inbox_relay_lookup",
            "dm_inbox_relay_lookup",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.dm_inbox_relays_slot.lock() {
            self.record_slot_decision("dm_inbox_relay_lookup", "dm_inbox_relay_lookup", true);
            *slot = lookup;
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// ADR-0070 PR 2 — install the kernel's
    /// [`nmp_core::substrate::ProfileLookup`] handle. The per-app crate
    /// (today `explicit_composition` register_substrate) hands in a concrete
    /// `nmp_nip01::ProfileCache`; the same `Arc` is the writer side fed by the
    /// kind:0 `Kind0Parser` registered above + the reader side the kernel
    /// exposes through `profile_lookup()`.
    ///
    /// MUST be called before `nmp_app_start` AND before any kind:0 event is
    /// ingested (the caches are independent stores; a late swap would lose
    /// entries written into the old cache).
    pub fn set_profile_lookup(
        &self,
        lookup: std::sync::Arc<dyn nmp_core::substrate::ProfileLookup>,
    ) -> NmpConfigStatus {
        if let Err(status) =
            self.ensure_prestart_config("profile_lookup", "profile_lookup", "profile_lookup")
        {
            return status;
        }
        if let Ok(mut slot) = self.composition.profile_lookup_slot.lock() {
            self.record_slot_decision("profile_lookup", "profile_lookup", true);
            *slot = lookup;
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// #2788 — install the protocol-owned contact-list reader that the kernel
    /// uses for active-contact transition detection and follow-edit gates.
    ///
    /// MUST be called before `nmp_app_start` so the actor snapshots the reader
    /// into every constructed kernel. The concrete implementation lives in
    /// `nmp-nip02`; native runtime only stores the opaque reader handle.
    pub fn set_contact_list_reader(
        &self,
        reader: std::sync::Arc<dyn nmp_core::slots::ContactListReader>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "contact_list_reader",
            "contact_list_reader",
            "contact_list_reader",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.contact_list_reader_slot.lock() {
            self.record_slot_decision("contact_list_reader", "contact_list_reader", true);
            *slot = reader;
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Install the kernel's [`nmp_core::substrate::BlockedRelayLookup`]
    /// handle. Mirrors [`Self::set_dm_inbox_relay_lookup`]: the per-app
    /// crate hands in a concrete `Arc<InMemoryBlockedRelayCache>` (from
    /// `nmp-router`); the same `Arc` is simultaneously the writer side
    /// fed by `nmp_router::Kind10006Parser` (registered through
    /// [`Self::register_ingest_parser`]) and the reader side the kernel
    /// snapshots through `build_routing_context`'s blocked-set
    /// post-pass.
    ///
    /// MUST be called before `nmp_app_start` AND before any kind:10006
    /// event is ingested (the caches are independent stores; a late swap
    /// would lose entries written into the old cache).
    pub fn set_blocked_relay_lookup(
        &self,
        lookup: std::sync::Arc<dyn nmp_core::substrate::BlockedRelayLookup>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "blocked_relay_lookup",
            "blocked_relay_lookup",
            "blocked_relay_lookup",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.blocked_relays_slot.lock() {
            self.record_slot_decision("blocked_relay_lookup", "blocked_relay_lookup", true);
            *slot = lookup;
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Install the protocol-owned external-id validator used by raw `i:<id>`
    /// event reference keys.
    pub fn set_external_id_validator(
        &self,
        validator: std::sync::Arc<dyn nmp_core::substrate::ExternalIdValidator>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "external_id_validator",
            "external_id_validator",
            "external_id_validator",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.external_id_validator_slot.lock() {
            self.record_slot_decision("external_id_validator", "external_id_validator", true);
            *slot = Some(validator);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// H4 — install the read-only [`nmp_core::substrate::MailboxCache`] handle
    /// the UniFFI NIP-19 `encode_profile` helper reads kind:10002 relay
    /// hints from. Mirrors [`Self::set_blocked_relay_lookup`] /
    /// [`Self::set_dm_inbox_relay_lookup`]: a poisoned lock is a silent no-op
    /// (D6).
    ///
    /// The `Arc` passed here MUST be the SAME `InMemoryMailboxCache` instance
    /// the app crate hands the routing-substrate factory and (critically) the
    /// `nmp_router::Kind10002Parser`. Instance identity is load-bearing: the
    /// encoder prefers `nprofile` only when it can read the relay hints the
    /// parser wrote on kind:10002 ingest. A fresh / divergent instance leaves
    /// the encoder reading an always-empty cache, silently degrading every
    /// result to a bare `npub`.
    ///
    /// MUST be called before `nmp_app_start` for the encoder to see relay
    /// hints ingested after start; the slot may be read on any thread (the
    /// encoder is a synchronous cache read, no actor round-trip).
    pub fn set_mailbox_cache_reader(
        &self,
        cache: std::sync::Arc<dyn nmp_core::substrate::MailboxCache>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "mailbox_cache_reader",
            "mailbox_cache_reader",
            "mailbox_cache_reader",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.mailbox_cache_reader.lock() {
            self.record_slot_decision(
                "mailbox_cache_reader",
                "mailbox_cache_reader",
                slot.is_some(),
            );
            *slot = Some(cache);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Snapshot the installed NIP-19-encoder mailbox-cache handle, if any.
    /// Returns a clone of the `Arc` so the caller can read it without holding
    /// the lock across the `write_relays` call. A poisoned lock reads as
    /// `None` (D6 — the encoder then falls back to `npub`).
    #[must_use]
    pub fn mailbox_cache_reader(
        &self,
    ) -> Option<std::sync::Arc<dyn nmp_core::substrate::MailboxCache>> {
        self.composition
            .mailbox_cache_reader
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }

    /// Override the active-account bootstrap Tailing self-kinds list.
    /// Passing `None` clears the override (the kernel reverts to its
    /// built-in `[0, 3, 10002, 10006, 10007]` default; kind:10000 is
    /// intentionally absent — owned by `MuteRuntimeController`).
    ///
    /// MUST be called BEFORE `nmp_app_start` so the actor binds the
    /// override onto the kernel at construction time, before the first
    /// `active_account_bootstrap_requests` call. A late write after sign-in
    /// is silently ignored — the bootstrap REQ shape is fixed at the
    /// moment of sign-in.
    pub fn set_bootstrap_self_kinds(&self, kinds: Option<Vec<u64>>) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "bootstrap_self_kinds",
            "bootstrap_self_kinds",
            "bootstrap_self_kinds",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.bootstrap_self_kinds.lock() {
            self.record_slot_decision(
                "bootstrap_self_kinds",
                "bootstrap_self_kinds",
                slot.is_some(),
            );
            *slot = kinds;
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// V-51 phase 5 — install the per-app substrate-routing factory.
    ///
    /// `factory` is a `Send + Sync` closure that, given the kernel's
    /// `RoutingTraceObserver` (the kernel-owned
    /// `Arc<RoutingTraceProjection>` clone), returns the
    /// `(Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)` pair the actor
    /// installs via [`crate::kernel::Kernel::set_routing`]. Production
    /// composition (`nmp-app-chirp`) writes a closure constructing
    /// `nmp_router::GenericOutboxRouter` (with the observer threaded
    /// through `with_trace_observer`) + `nmp_router::InMemoryMailboxCache`.
    ///
    /// MUST be called BEFORE `nmp_app_start` AND BEFORE any kind:10002
    /// event is ingested. The substrate caches the kernel holds and the
    /// production `nmp_router::InMemoryMailboxCache` are independent
    /// stores, not a write-through pair — a swap after ingest would lose
    /// the cached entries. `D6`: a poisoned slot is a silent no-op (the
    /// kernel keeps its in-crate defaults; the production swap is a
    /// no-op for that one process).
    ///
    /// The snapped factory is re-invoked by the `Reset` dispatch arm against
    /// the rebuilt kernel's fresh projection so the production router/cache
    /// pair survives a state wipe (mirrors the `routing_trace_slot` re-publish
    /// step).
    pub fn set_routing_substrate<F>(&self, factory: F) -> NmpConfigStatus
    where
        F: Fn(
                std::sync::Arc<dyn nmp_core::substrate::RoutingTraceObserver>,
            ) -> (
                std::sync::Arc<dyn nmp_core::substrate::OutboxRouter>,
                std::sync::Arc<dyn nmp_core::substrate::MailboxCache>,
            ) + Send
            + Sync
            + 'static,
    {
        if let Err(status) = self.ensure_prestart_config(
            "routing_substrate",
            "routing_substrate",
            "routing_substrate",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.routing_substrate.lock() {
            // ADR-0069 Part 2 — record the last-writer-wins decision.
            self.record_slot_decision("routing_substrate", "routing_substrate", slot.is_some());
            *slot = Some(std::sync::Arc::new(factory));
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Install the substrate-publish-resolver factory.
    ///
    /// Spec §271 (2026-05-25): `Nip65OutboxResolver` was moved out of
    /// `nmp-core::publish::nip65` into `nmp-router` (Layer 2). The kernel
    /// constructs `PublishEngine` with the in-crate `NoopOutboxResolver`
    /// default (fail-closed); production composition installs the router-side
    /// resolver through this factory slot. Actor startup snapshots
    /// the slot and invokes [`crate::kernel::Kernel::set_publish_resolver`]
    /// with the produced `Arc<dyn OutboxResolver>`; `Reset` re-invokes the
    /// same snapped factory against fresh kernel handles.
    ///
    /// `factory` receives the kernel-owned handles publish resolvers may need.
    /// Production `Nip65OutboxResolver` reads the same `MailboxCache` instance
    /// that `Kind10002Parser` writes; `EventStore` remains available for other
    /// resolver implementations. D4 holds because the actor remains the sole
    /// writer for the shared slots and parser-dispatched cache updates.
    ///
    /// MUST be called BEFORE `nmp_app_start` AND BEFORE any kind:10002
    /// event is ingested. `D6`: a poisoned slot is a silent no-op (the
    /// kernel keeps its `NoopOutboxResolver`; every publish then fails
    /// closed with `NoTargets`).
    ///
    /// The snapped factory is re-invoked by the `Reset` dispatch arm against
    /// the rebuilt kernel's fresh handles so the production resolver survives
    /// a state wipe (mirrors `set_routing_substrate`).
    pub fn set_publish_resolver_factory<F>(&self, factory: F) -> NmpConfigStatus
    where
        F: Fn(
                std::sync::Arc<dyn nmp_store::EventStore>,
                std::sync::Arc<dyn nmp_core::substrate::MailboxCache>,
                nmp_core::slots::IndexerRelaysSlot,
                nmp_core::slots::LocalWriteRelaysSlot,
                nmp_core::slots::ActiveAccountSlot,
            ) -> std::sync::Arc<dyn nmp_core::publish::OutboxResolver>
            + Send
            + Sync
            + 'static,
    {
        if let Err(status) =
            self.ensure_prestart_config("publish_resolver", "publish_resolver", "publish_resolver")
        {
            return status;
        }
        if let Ok(mut slot) = self.composition.publish_resolver.lock() {
            self.record_slot_decision("publish_resolver", "publish_resolver", slot.is_some());
            *slot = Some(std::sync::Arc::new(factory));
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Install router-owned relay-list publish support before actor start.
    pub fn set_relay_list_publish_support(
        &self,
        support: std::sync::Arc<dyn nmp_core::slots::RelayListPublishSupport>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "relay_list_publish_support",
            "relay_list_publish_support",
            "relay_list_publish_support",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.relay_list_publish_support.lock() {
            *slot = support;
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Test-support: install a deterministic kernel clock BEFORE
    /// `nmp_app_start`. The actor applies it via `Kernel::set_clock` at
    /// construction, so every event the kernel stamps thereafter reads its
    /// `created_at` from this clock. Tests that publish two replaceable events
    /// (e.g. a kind:3 follow then unfollow) advance the shared clock between
    /// dispatches so the second event wins the NIP-01 replaceable supersession
    /// deterministically — no wall-clock sleep (D8). Production never calls
    /// this (the slot stays `None`; the kernel keeps its `SystemClock`).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_kernel_clock_for_test(&self, clock: std::sync::Arc<nmp_core::MonotonicSecondClock>) {
        if let Ok(mut slot) = self.composition.kernel_clock.lock() {
            *slot = Some(nmp_core::slots::erase_kernel_clock(clock));
        }
    }
}
