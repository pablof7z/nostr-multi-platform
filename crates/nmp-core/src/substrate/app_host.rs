//! App-host registration seams.
//!
//! Reusable protocol and routing crates must not depend on the native C-ABI
//! crate just to wire their modules into an application. These traits live at
//! the substrate layer so crates can register actions, parsers, observers, and
//! runtime projections against any host that implements the same Rust contract.
//! `nmp-ffi::NmpApp` is one implementation, not the type every reusable crate
//! has to name.

use std::ops::Range;
use std::sync::Arc;

use crate::publish::OutboxResolver;
use crate::slots::{
    ActiveAccountSlot, ActiveLocalKeysSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use crate::store::EventStore;
use crate::subs::PlanCoverageHook;
use crate::update_envelope::TypedProjectionData;
use crate::{
    AppRelaySlot, KernelEventObserver, KernelEventObserverId, KindFilter, RawEventObserver,
    RawEventObserverId,
};

use super::{
    ActionRegistrar, DmInboxRelayLookup, IngestParser, MailboxCache, OutboxRouter,
    RawEventForwardPolicy, RawEventForwardPolicyContext, RelayConnectedHook, RelayTextInterceptor,
    ReqFrameInterceptor, RoutingTraceObserver,
};

/// Host surface needed by reusable NMP composition crates.
///
/// This is intentionally a Rust trait rather than an FFI handle. Protocol
/// crates can depend on `nmp-core`, register their substrate pieces, and leave
/// the actual host implementation to `nmp-ffi` or another embedding layer.
pub trait AppHost: ActionRegistrar {
    fn register_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static;

    /// Register a **change-gated** snapshot projection — the perf-aware
    /// counterpart to [`AppHost::register_snapshot_projection`].
    ///
    /// Identical to the ungated variant except the closure is only re-invoked
    /// when `gate`'s value has advanced since the previous snapshot tick for
    /// this `key`. On a tick where the gate is unchanged, the registry returns
    /// the value the closure last produced (cloned from a per-key memo) WITHOUT
    /// calling the closure.
    ///
    /// This is the fix for the "re-serialize the whole app library to JSON on
    /// every emit" hot path: the registry previously ran every projection on
    /// every `make_update`, so any unrelated kernel emit (an incoming relay
    /// event, a tick) forced a multi-MB serializer to re-run. A host passes the
    /// `Arc<AtomicU64>` rev it already bumps on data mutation as the `gate`, and
    /// the serializer only runs when the rev advances.
    ///
    /// `gate` is any [`ChangeGate`](crate::kernel::ChangeGate); an
    /// `Arc<AtomicU64>` rev counter is the canonical choice
    /// ([`AtomicU64`](std::sync::atomic::AtomicU64) implements `ChangeGate`).
    /// Last-writer-wins by `key`, exactly like the ungated variant. Like the
    /// generic closure, `f` runs on the actor thread inside the snapshot tick —
    /// it MUST be non-blocking (D8).
    fn register_snapshot_projection_gated<K, F>(
        &self,
        key: K,
        gate: Arc<dyn crate::kernel::ChangeGate>,
        f: F,
    ) where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static;

    /// Register a **typed** FlatBuffers projection closure under `key` — the
    /// typed-sidecar counterpart to [`AppHost::register_snapshot_projection`]
    /// (ADR-0037). The closure returns the projection's opaque, host-declared
    /// FlatBuffers payload ([`TypedProjectionData`]) carried verbatim in every
    /// `SnapshotFrame`'s `typed_projections` sidecar, or `None` when there is
    /// nothing to emit this tick.
    ///
    /// This method lives on the trait — not only on the concrete `NmpApp` — so
    /// reusable protocol/feed crates that register through `&impl AppHost`
    /// (e.g. `register_runtime`) can wire typed projections without depending
    /// on the C-ABI crate. It mirrors `register_snapshot_projection`: `&self`
    /// (the registry mutation is a lock-and-insert), and the same host-chosen
    /// key space shared with the generic registry (ADR-0037 Commitment 4).
    ///
    /// Like the generic closure, `f` runs on the actor thread inside the
    /// snapshot tick — it MUST be non-blocking (D8).
    fn register_typed_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<TypedProjectionData> + Send + Sync + 'static;

    /// Register a **per-tick observer** — a no-result callback fired once on
    /// every snapshot tick, the generic projection-free counterpart to
    /// [`AppHost::register_snapshot_projection`].
    ///
    /// Where a projection closure produces snapshot *data* under a key, a tick
    /// observer produces nothing: it is a pure per-tick side-effect seam for
    /// host-side reconcilers that need a "the kernel just ticked" callback but
    /// contribute no projection output. The canonical consumer is an
    /// active-account subscription reconciler that diffs the active pubkey each
    /// tick and enqueues `PushInterest` / `WithdrawInterest` actor commands —
    /// previously such reconcilers abused the projection registry by returning a
    /// `Value::Null` projection purely to obtain the per-tick callback.
    ///
    /// This method lives on the trait — not only on the concrete `NmpApp` — so
    /// reusable protocol/runtime crates that register through `&impl AppHost`
    /// (e.g. `register_zap_receipts_runtime`) can wire a per-tick reconciler
    /// without depending on the C-ABI crate. It mirrors
    /// `register_snapshot_projection`: `&self` (the registry mutation is a
    /// lock-and-push), and the same shared registry/slot.
    ///
    /// Like a projection closure, `f` runs on the actor thread inside the
    /// snapshot tick — it MUST be non-blocking (D8: enqueue only, no I/O or
    /// lock waits). A panicking observer is contained (D6) and cannot crash the
    /// tick.
    fn register_snapshot_tick_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static;

    /// ADR-0055 Rung 3 — declare that this host runtime owns the NMP
    /// cache-merge layer (D3-3) and is ready to receive frames with
    /// `Unchanged` projections omitted.
    ///
    /// Single-writer, set before `nmp_app_start`. After this call the kernel
    /// guarantees the NEXT `make_update` frame is a full baseline (all live
    /// Tier-2 projections emitted as `Changed`). Until this is called the
    /// kernel emits full rows on every tick (no behavior change for
    /// non-advertising hosts). Idempotent — calling multiple times is safe.
    ///
    /// This is durable architecture (the per-attach baseline gate + the
    /// Rung-5 ADR-0053 compose seam), NOT a compat shim.
    fn declare_incremental_apply(&self);

    /// ADR-0053 — declare the static set of **Tier-2 built-in projection keys**
    /// this host consumes (the union of every projection any of the app's screens
    /// can read, known at app build time).
    ///
    /// The output-side sibling of the relay `push_interest` lattice: the kernel
    /// serializes a kernel-owned built-in into each snapshot only if its key is
    /// in the declared set. An **empty** declared set means "no opinion" and
    /// emits every built-in (no narrowing — the relay-filter semantic, where an
    /// empty filter set does not subscribe to nothing). A **non-empty** set
    /// narrows the built-ins to its members, skipping the producer work (no
    /// serialize, no roll-up) for everything else — most notably the
    /// `relay_diagnostics` roll-up, which no longer ships to hosts that do not
    /// declare it.
    ///
    /// Additive (unions into the set) and `&self` (the mutation is a
    /// lock-and-extend behind the shared registry slot). Intended as a host-init
    /// call, before `nmp_app_start`. Tier-1 host/protocol projections registered
    /// via [`AppHost::register_snapshot_projection`] are NOT gated by this —
    /// registration already declares their consumption (and dynamic feeds gate by
    /// their `unregister_feed` lifecycle).
    fn declare_consumed_projections<I, K>(&self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>;

    fn set_coverage_hook(&self, hook: PlanCoverageHook);

    fn set_req_frame_interceptor(&self, interceptor: Arc<dyn ReqFrameInterceptor>);

    fn add_relay_text_interceptor(&self, interceptor: Arc<dyn RelayTextInterceptor>);

    /// ADR-0051 — install a [`RelayConnectedHook`] so a protocol crate (today
    /// `nmp-nip11`) reacts when a relay connects (e.g. fetch its NIP-11
    /// information document). Additive: multiple crates may react to the same
    /// connect.
    fn add_relay_connected_hook(&self, hook: Arc<dyn RelayConnectedHook>);

    fn register_ingest_parser(&self, kind: u32, parser: Arc<dyn IngestParser>);

    /// Slot-keyed replace: evict the prior parser registered under `slot_key`
    /// for `kind` (if any), then install `parser` under the same slot. Parsers
    /// registered under **other** slot keys (or via [`Self::register_ingest_parser`]
    /// with no slot key) are untouched.
    ///
    /// Used by lifecycle-managed singleton seams — each caller owns a unique
    /// `slot_key` (e.g. `"nip17.dm_inbox"` or `"marmot"`) and re-registrations
    /// only evict the caller's own prior entry. Multiple lifecycle-managed parsers
    /// on the same kind (e.g. the NIP-17 DM inbox and Marmot on kind:1059)
    /// coexist safely because they own distinct slots.
    ///
    /// Returns the previous parser for `(kind, slot_key)`, or `None` when this is
    /// the first registration for that slot. D6 — a poisoned dispatcher lock is a
    /// silent no-op returning `None` (the registration is dropped; existing parsers
    /// are preserved).
    ///
    /// **Slot keys MUST be globally unique across crates.** A second component
    /// reusing an existing slot name silently evicts the peer's parser. Choose a
    /// fully-qualified reverse-domain key (e.g. `"nip17.dm_inbox"`, `"marmot"`)
    /// that cannot collide with any other crate's registration.
    fn replace_ingest_parser(
        &self,
        kind: u32,
        slot_key: &'static str,
        parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>>;

    /// Remove the parser registered under `slot_key` for `kind`, if any.
    ///
    /// Used by teardown paths (e.g. Marmot sign-out without re-register) to
    /// clear a lifecycle-managed slot. D6 — a poisoned dispatcher lock is a
    /// silent no-op.
    fn unregister_ingest_parser(&self, kind: u32, slot_key: &'static str);

    /// Slot-keyed replace for a kind range: evict the prior range-parser
    /// registered under `slot_key` (if any), then install `parser` covering
    /// `range`. Parsers registered under other slot keys or via the slot-less
    /// [`Self::register_ingest_parser`] are untouched.
    ///
    /// Used by parsers that need to receive every kind (e.g. an all-kinds
    /// debug raw-event cache). Returns the previous parser for `slot_key`, or
    /// `None` when this is the first registration for that slot. D6 — a
    /// poisoned dispatcher lock is a silent no-op returning `None`.
    ///
    /// **Slot keys MUST be globally unique across crates.** Choose a
    /// fully-qualified reverse-domain key (e.g. `"chirp-tui.raw-cache"`) that
    /// cannot collide with any other crate's registration.
    fn replace_ingest_parser_range(
        &self,
        range: Range<u32>,
        slot_key: &'static str,
        parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>>;

    /// Remove the range-parser registered under `slot_key`, if any. D6 — a
    /// poisoned dispatcher lock is a silent no-op.
    fn unregister_ingest_parser_range(&self, slot_key: &'static str);

    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn DmInboxRelayLookup>);

    /// H4 — install the read-only [`MailboxCache`] handle the host's NIP-19
    /// identity encoder (`nmp_app_encode_profile`) reads kind:10002 relay
    /// hints from. The composition root passes the SAME `MailboxCache`
    /// instance it hands [`AppHost::set_routing_substrate`] and the
    /// kind:10002 [`IngestParser`], so the encoder can prefer `nprofile` over
    /// a bare `npub` using the hints the parser writes on ingest. Read-only,
    /// synchronous — no network, no actor round-trip.
    fn set_mailbox_cache_reader(&self, cache: Arc<dyn MailboxCache>);

    fn set_routing_substrate<F>(&self, factory: F)
    where
        F: Fn(Arc<dyn RoutingTraceObserver>) -> (Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)
            + Send
            + Sync
            + 'static;

    fn set_publish_resolver_factory<F>(&self, factory: F)
    where
        F: Fn(
                Arc<dyn EventStore>,
                IndexerRelaysSlot,
                LocalWriteRelaysSlot,
                ActiveAccountSlot,
            ) -> Arc<dyn OutboxResolver>
            + Send
            + Sync
            + 'static;

    fn set_raw_event_forward_policy_factory<F>(&self, factory: F)
    where
        F: Fn(RawEventForwardPolicyContext) -> Vec<Arc<dyn RawEventForwardPolicy>>
            + Send
            + Sync
            + 'static;

    fn active_local_keys(&self) -> ActiveLocalKeysSlot;

    /// Pubkey-only identity accessor — least-privilege sibling of
    /// [`Self::active_local_keys`].
    ///
    /// Returns the SAME shared [`ActiveAccountSlot`] (`Arc<Mutex<Option<String>>>`,
    /// hex pubkey) the kernel actor writes on every identity mutation. Unlike
    /// [`Self::active_local_keys`] — which exposes the full `nostr::Keys` and is
    /// therefore `None` for remote-signer (NIP-46 bunker) accounts whose secret
    /// material lives outside the kernel — this slot is populated for **every**
    /// backend, including bunker. Identity-only consumers (WOT bootstrap, the
    /// DM relay-list runtime, self-zap-receipt and mute-list reconcilers) MUST
    /// read this so they activate for bunker accounts; only consumers that
    /// genuinely need secret key material (signing, NIP-44 unseal) stay on
    /// `active_local_keys()`.
    ///
    /// Single source of truth (D4): this is the exact slot the actor populates
    /// in `kernel::identity_state` — it is not a second mirror of the active
    /// account. `None` means no account is signed in.
    fn active_pubkey(&self) -> ActiveAccountSlot;

    fn actor_sender(&self) -> crate::actor::CommandSender;

    fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId;

    fn unregister_event_observer(&self, id: KernelEventObserverId);

    fn swap_singleton_event_observer(
        &self,
        new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId>;

    /// Register a raw signed-event observer for **verbatim forwarding only**.
    ///
    /// The tap delivers the exact signed NIP-01 frame (including `sig`) for
    /// every accepted live-ingest event matching `kinds`. It fires on live
    /// ingest (including `Duplicate` outcomes) but does **NOT** fire on
    /// cache-served replay.
    ///
    /// **State derivation belongs on `register_ingest_parser` (rule A5),
    /// not here.** The `IngestParser` seam fires on cache-served replay
    /// (since PR-1/#1137 + PR-2/#1145) and supports slot-keyed replace for
    /// lifecycle-managed singleton parsers. Use the raw tap exclusively when
    /// the `sig` field must be forwarded verbatim to an external store or
    /// relay bridge (e.g. the `hl` app's nostrdb mirror).
    fn register_raw_event_observer(
        &self,
        kinds: KindFilter,
        observer: Arc<dyn RawEventObserver>,
    ) -> RawEventObserverId;

    fn unregister_raw_event_observer(&self, id: RawEventObserverId);

    fn configured_relays_handle(&self) -> AppRelaySlot;

    /// Register the host-supplied fallback relay URL for client-initiated
    /// NIP-46 `nostrconnect://` handshakes.
    ///
    /// Must be called before `nmp_app_start`. The composition root
    /// (`nmp_defaults::register_defaults`) supplies a sane default; a
    /// per-app crate may override it. When no URL has been registered the
    /// substrate surfaces a typed error rather than silently using a hardcoded
    /// URL (V-65 / D0).
    fn set_nostrconnect_bootstrap_relay(&self, url: String);

    /// Register a Rust-side callback for active-account changes.
    ///
    /// The callback runs on the update-listener thread after the actor has
    /// written [`Self::active_local_keys`] and emitted an update frame. It
    /// fires only when the slot value changes (`Some(pubkey)` on sign-in /
    /// switch, `None` on logout / reset), never on ordinary snapshot ticks.
    /// This is the canonical composition seam for long-lived Rust objects that
    /// need to reset per-account state without polling.
    ///
    /// The callback receives the new active pubkey (hex), or `None` on
    /// logout / reset. No unregister is provided — current consumers are
    /// app-lifetime registrations installed during host init.
    ///
    /// This method lives on the trait — not only on the concrete `NmpApp` — so
    /// reusable protocol/runtime crates that register through `&impl AppHost`
    /// can wire per-account lifecycle hooks without depending on the C-ABI crate.
    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static;
}
