//! Command + relay-event dispatch reducers.
//!
//! Split out of `mod.rs` to keep both files under the 300-LOC soft cap.
//! `dispatch_command` resolves an [`ActorCommand`] into outbound relay
//! messages (or `None` for shutdown); `handle_relay_event` folds a
//! [`nmp_network::pool::PoolEvent`] (phase F rename of the legacy
//! `RelayEvent`) into the kernel + connection bookkeeping. No behavior
//! change — the actor's per-URL bookkeeping, reconnect-replay, and
//! startup-send gating are all preserved one-to-one across the rename.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zeroize::Zeroizing;

use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole};
use crate::slots::{ActiveLocalKeysSlot, MlsLocalNsecSlot};
use crate::substrate::HostOpHandlerSlot;
use nmp_network::pool::{BackoffClass, Pool, PoolEvent, RelayFrame as PoolFrame};

use crate::kernel::{BackoffHint, RelayFrame};

/// Convert a [`nmp_network::pool::RelayFrame`] (the wire frame variant the
/// pool's translator emits) into the kernel's wire-transport-agnostic
/// [`RelayFrame`] consumed by `Kernel::handle_message`.
///
/// Step 8 phase F: replaces the prior `tungstenite::Message → RelayFrame`
/// adapter — the pool already owns that conversion in its translator thread,
/// so this adapter is now a pure variant-rename (1:1 mapping). The
/// [`PoolFrame::Auth`] variant (phase E pre-classification) is round-tripped
/// to `RelayFrame::Text` by reconstructing the canonical
/// `["AUTH", <challenge>]` text frame; the kernel's existing
/// `auth_handlers.rs` ingest path then sees an unchanged surface.
/// `nmp-network`'s `nmp-nip42-types` parser already validated the shape on
/// the way in, so the round-trip is structural.
fn pool_frame_to_relay_frame(frame: PoolFrame) -> RelayFrame {
    match frame {
        PoolFrame::Text(text) => RelayFrame::Text(text),
        PoolFrame::Auth(challenge) => {
            // Reconstruct the canonical NIP-42 wire shape so the kernel
            // ingest's existing `["AUTH", ...]` parse path handles it
            // unchanged (the wire-layer pre-classification is opportunistic;
            // the kernel still owns the AUTH state machine).
            let payload = serde_json::json!(["AUTH", challenge]).to_string();
            RelayFrame::Text(payload)
        }
        PoolFrame::Binary(bytes) => RelayFrame::Binary(bytes),
        PoolFrame::Ping => RelayFrame::Ping,
        PoolFrame::Pong => RelayFrame::Pong,
        PoolFrame::Close(reason) => RelayFrame::Close(reason),
    }
}
use crate::subs::PlanCoverageHook;

use super::capability_worker::CapabilityWorkSender;
use super::commands::{self, IdentityRuntime, LifecycleObserverSlot};
use super::pending_sign::{PendingSign, PendingSignReturn};
use super::relay_mgmt::{
    close_relays, ensure_relay_worker, maybe_send_startup, send_all_outbound,
    shutdown_relay_worker, spawn_missing_relays,
};
use super::session_persistence;
use super::tick::{emit_now, maybe_emit_after_dispatch};
use super::{ActorCommand, RelayControl};
use crate::capability_socket::CapabilityCallbackSlot;
use crate::kernel_action::dispatch_kernel_action;

/// Sync every host-readable local-key mirror to the current active account.
///
/// Two parallel substrate-generic slots track the active account's local
/// signing material on every identity mutation:
///
/// * `mls_local_nsec` — bech32 `nsec1…` wrapped in [`Zeroizing`] so the
///   previous string is wiped from the heap on overwrite.
/// * `active_local_keys` — the parsed `nostr::Keys`. `Keys` zeroizes its own
///   secret on drop, so no extra wrapper is needed.
///
/// Both derive from `identity.active_keys()`, so they always change together.
/// The substrate publishes both unconditionally; non-substrate consumers
/// (FFI-shell readers exposed via `NmpApp::active_local_keys`) decide what
/// to do with the data (today: NIP-17 gift-wrap unsealing, NIP-57 zap
/// receipt pubkey reads). Each slot is locked, written, and dropped
/// sequentially — there is no cross-slot atomicity contract (a host that
/// races a snapshot read against an identity switch may briefly observe one
/// slot updated and the other not; the next snapshot tick reconciles).
///
/// Called synchronously BEFORE `maybe_emit_after_dispatch` (and before
/// `emit_now` on the `Start` arm) so the slots are visible to host callbacks
/// before any snapshot fires.
fn update_local_key_slots(
    identity: &IdentityRuntime,
    nsec_slot: &MlsLocalNsecSlot,
    keys_slot: &ActiveLocalKeysSlot,
) {
    if let Ok(mut guard) = nsec_slot.lock() {
        *guard = identity.active_nsec_bech32().map(Zeroizing::new);
    }
    if let Ok(mut guard) = keys_slot.lock() {
        *guard = identity.active_local_keys().cloned();
    }
}

/// Re-publish the active account's NIP-65 kind:10002 relay list after an
/// `AddRelay` / `RemoveRelay` mutation, so other clients reading the relay
/// graph see the same set the user just edited.
///
/// # Why
///
/// Before this hook, the actor's `AddRelay` / `RemoveRelay` arms mutated
/// the local `AppRelay` projection and dialed / dropped sockets, but
/// never re-published the user's NIP-65 outbox. The asymmetric leak:
/// removing a defunct relay never told other clients to stop fanning out
/// to it; adding a new relay never told contacts to read/write there. The
/// `nmp.nip65.publish_relay_list` action (`nmp-router` crate) closes the
/// host-dispatched half of the loop; this helper closes the actor-internal
/// half so the FFI `nmp_app_add_relay` / `nmp_app_remove_relay` paths and
/// any non-action caller of those `ActorCommand`s also keep NIP-65 in
/// sync.
///
/// # Skip semantics — three guards
///
/// 1. **No active account.** A relay edit while signed out is a local
///    settings change; there is no identity to sign under. `publish_unsigned_event`
///    would otherwise set an error toast via `toast_no_account`, which is
///    the wrong observable for a config edit.
/// 2. **Projection unchanged.** Re-adding an already-present URL with the
///    same role, or removing a URL that was never present, leaves the
///    projection identical to its prior state. Republishing kind:10002
///    in that case would waste a write and bump the timestamp for no
///    behavioural change. `projection_before` is the snapshot the caller
///    took *before* the local mutation; equality means "no semantic change".
/// 3. **No NIP-65-eligible rows.** A projection containing only pure-indexer
///    rows (or one that becomes empty after the edit) cannot produce a
///    kind:10002 with `r` tags. `build_relay_list_event`
///    returns `None` in that case, and the function bails before any
///    publish — an empty kind:10002 is the destructive "clear my NIP-65
///    metadata" signal in `ingest_relay_list`, and we must never emit
///    that as a side effect of a relay edit.
///
/// # `correlation_id`
///
/// `None` — these are actor-internal publishes piggybacked onto a local
/// mutation, not action-dispatched. Hosts that *want* an observable
/// terminal verdict dispatch `nmp.nip65.publish_relay_list` directly,
/// which threads a registry-minted id through `PublishUnsignedEvent`.
///
/// # `created_at`
///
/// D7 sentinel: the builder sets `created_at = 0`; the actor's
/// `PublishUnsignedEvent` arm re-stamps it from the kernel clock. This
/// function never reads the system clock.
fn maybe_publish_relay_list_after_edit(
    identity: &commands::IdentityRuntime,
    kernel: &mut Kernel,
    projection_before: &[crate::kernel::AppRelay],
    pending_signs: &mut Vec<super::pending_sign::PendingSign>,
) -> Vec<OutboundMessage> {
    // Guard 1: must have an active signer.
    if identity.active_pubkey().is_none() {
        return Vec::new();
    }
    // Guard 2: skip on no-op projection change.
    let projection_after = kernel.configured_relays_snapshot();
    if projection_after == projection_before {
        return Vec::new();
    }
    // Guard 3: skip when the projection has no NIP-65 expression.
    let Some(unsigned) = commands::build_relay_list_event(projection_after) else {
        return Vec::new();
    };
    commands::publish_unsigned_event(identity, kernel, unsigned, None, None, pending_signs)
}

/// Parse a host sign-and-return draft into an [`crate::substrate::UnsignedEvent`].
///
/// The draft is `{ "kind": u64, "content": str, "tags": [[str, …], …],
/// "created_at": u64? }` — the shape `nmp_app_sign_event_for_return` accepts.
/// It carries NO `pubkey` (the host does not know which signer will be used)
/// and its `created_at` is advisory, so this helper fills both:
///
/// * `pubkey` ← the resolved signer's hex pubkey.
/// * `created_at` ← the kernel clock (`now_secs`, D7) — the host never owns
///   wall-clock time; any `created_at` in the draft is ignored.
///
/// `tags` defaults to empty when absent. `kind` and `content` are required.
fn build_unsigned_for_return(
    unsigned_json: &str,
    signer_pubkey: &str,
    now_secs: u64,
) -> Result<crate::substrate::UnsignedEvent, String> {
    let value: serde_json::Value =
        serde_json::from_str(unsigned_json).map_err(|e| e.to_string())?;
    let kind = value
        .get("kind")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "missing or non-integer `kind`".to_string())?;
    let kind = u32::try_from(kind).map_err(|_| "`kind` out of u32 range".to_string())?;
    let content = value
        .get("content")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "missing or non-string `content`".to_string())?
        .to_string();
    let tags: Vec<Vec<String>> = match value.get("tags") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(tags_value) => serde_json::from_value(tags_value.clone())
            .map_err(|e| format!("`tags` must be an array of string arrays: {e}"))?,
    };
    Ok(crate::substrate::UnsignedEvent {
        pubkey: signer_pubkey.to_string(),
        kind,
        tags,
        content,
        created_at: now_secs,
    })
}

/// Serialize a [`crate::substrate::SignedEvent`] into the standard flat Nostr
/// event JSON: `{ "id", "pubkey", "created_at", "kind", "tags", "content",
/// "sig" }`. This is the on-wire NIP-01 event object (the inner body of an
/// `["EVENT", …]` frame), which is what a host base64-encodes for a Blossom
/// `Authorization: Nostr …` header. NOT the kernel-internal `SignedEvent`
/// serde shape (which nests under `unsigned`).
///
/// `pub(super)` so the idle-loop `pending_sign_returns` drain in `mod.rs`
/// reuses the exact same flat-event serialization the dispatch arm uses.
pub(super) fn signed_event_to_json(signed: &crate::substrate::SignedEvent) -> String {
    // Delegates to the public `SignedEvent::to_nip01_json` so the flat-event
    // serialization has exactly one definition shared by the dispatch arm, the
    // idle-loop drain, and protocol-crate workers.
    signed.to_nip01_json()
}

/// Borrowed bundle of the actor loop's mutable runtime state.
///
/// Replaces the 15+ explicit parameters that `dispatch_command` used to take.
/// Constructed fresh per command in `run_actor_with_observers` and dropped
/// immediately after dispatch, so every other call site in the actor loop
/// keeps using the original locals untouched. The lifetime `'a` ties the
/// struct to those stack-resident locals — no heap allocation, no ownership
/// transfer, the actor loop still owns every field.
///
/// Field access in `dispatch.rs` is always direct (`ctx.kernel`,
/// `&mut ctx.relay_controls`) so the borrow checker sees disjoint borrows;
/// no `impl` method should hold multiple `&mut` field borrows at once.
pub(super) struct ActorContext<'a> {
    pub(super) kernel: &'a mut Kernel,
    pub(super) identity: &'a mut IdentityRuntime,
    pub(super) relay_controls: &'a mut HashMap<CanonicalRelayUrl, RelayControl>,
    /// Phase F: side-map from `RelayHandle.slot()` → canonical URL so an
    /// inbound [`PoolEvent`] (which carries the handle but not always the
    /// URL) resolves back to `relay_controls` in O(1).
    pub(super) slot_to_url: &'a mut HashMap<u32, CanonicalRelayUrl>,
    /// Phase F: the push-model relay-connection pool. Cheap to clone, but the
    /// borrow is sufficient for dispatch — the actor loop owns the master
    /// handle for the whole process.
    pub(super) pool: &'a Pool,
    pub(super) connected_relays: &'a mut HashSet<RelayRole>,
    pub(super) connected_urls: &'a mut HashSet<CanonicalRelayUrl>,
    pub(super) update_tx: &'a Sender<crate::update_envelope::UpdateFrameBytes>,
    pub(super) last_emit: &'a mut Instant,
    pub(super) next_relay_generation: &'a mut u64,
    pub(super) running: &'a mut bool,
    pub(super) emit_hz: &'a mut u32,
    pub(super) startup_sent: &'a mut bool,
    /// Derived per-call value (`all_relays_connected(...)`), not a borrow.
    pub(super) relays_ready: bool,
    pub(super) lifecycle_observer: &'a LifecycleObserverSlot,
    pub(super) mls_local_nsec: &'a MlsLocalNsecSlot,
    /// Substrate-generic active-account local-keys slot — the active
    /// account's `nostr::Keys`, parallel in shape to `mls_local_nsec` and
    /// written together by [`update_local_key_slots`] on every identity
    /// mutation. The substrate names no NIP; non-substrate consumers
    /// (today: `nmp-nip17` gift-wrap unsealing via `DmInboxProjection`,
    /// `nmp-nip57` zap-receipt subscription) read the same `Arc` clone
    /// through the FFI shell's `NmpApp::active_local_keys` accessor.
    pub(super) active_local_keys: &'a ActiveLocalKeysSlot,
    pub(super) capability_callback: &'a CapabilityCallbackSlot,
    pub(super) pending_signs: &'a mut Vec<PendingSign>,
    /// Sign-and-return ops (`SignEventForReturn`) whose signer went `Pending`
    /// (NIP-46 bunker). Parked here and `poll()`ed on the idle loop exactly
    /// like `pending_signs`, but resolved into the `signed_events` snapshot
    /// projection instead of the publish engine. Local-key signs resolve
    /// inline in the dispatch arm and never reach this vec.
    pub(super) pending_sign_returns: &'a mut Vec<PendingSignReturn>,
    /// Self-feedback `Sender<ActorCommand>` — the actor's own command channel
    /// from the perspective of code running on the actor thread.
    /// `dispatch.rs` arms that spawn background workers (the LNURL-pay
    /// HTTP round-trip dispatched via `ActorCommand::Protocol` carries an
    /// owned clone through `ProtocolCommandContext::command_sender_clone`)
    /// clone this and hand the clone to the worker; the worker then sends
    /// a follow-up `ActorCommand` (e.g. `ShowToast` with the bolt11
    /// invoice) back into the actor loop without needing access to the
    /// `NmpApp`.
    ///
    /// D8 — the actor never `recv`s on this sender; it only hands clones
    /// out. The matching receiver is `command_rx` in `run_actor_with_observers`.
    /// A disconnected sender (post-Shutdown) is a benign send-failure on
    /// the worker side; the worker swallows it as a no-op (D6).
    pub(super) command_tx_self: &'a Sender<crate::actor::ActorCommand>,
    /// ADR-0040 §3 — sender half of the serialized capability-worker queue
    /// (V-90 Site 2). Identity-mutation dispatch arms enqueue a
    /// [`super::capability_worker::CapabilityWorkItem`] here instead of
    /// calling `dispatch_capability` inline; the single dedicated worker
    /// thread runs the synchronous native callback off the actor thread
    /// and re-enters via `ActorCommand::CapabilityResultReady`.
    ///
    /// D8 — the actor only *sends* to this channel; it never `recv`s.
    /// A disconnected channel (post-teardown) is a benign send-failure
    /// (D6 — the write is already irrelevant at that point).
    pub(super) capability_work_tx: &'a CapabilityWorkSender,
    /// D2 — coverage-gate hook slot. Read by the `Reset` arm to re-install
    /// the hook on the rebuilt kernel (mirrors initial install in
    /// `run_actor_with_observers`).
    pub(super) coverage_hook_slot: &'a Arc<Mutex<Option<PlanCoverageHook>>>,
    /// Outbound planner REQ interceptor slot. Read by the `Reset` arm to
    /// re-install the hook on the rebuilt kernel.
    pub(super) req_frame_interceptor_slot: &'a crate::substrate::ReqFrameInterceptorSlot,
    /// Host-installed [`crate::substrate::HostOpHandler`] slot. Read by the
    /// [`ActorCommand::DispatchHostOp`] arm to route the action body to the
    /// owner of the app-side state (today: `nmp-app-marmot`'s MLS service).
    /// `None` means no handler was installed before the dispatch — the arm
    /// records a `Failed` terminal stage for the correlation id.
    pub(super) host_op_handler: &'a HostOpHandlerSlot,
    /// V-40 — shared [`crate::substrate::EventIngestDispatcher`] slot.
    /// Read by the `Reset` arm to re-bind the slot onto the rebuilt
    /// kernel so per-NIP `register_actions` registrations survive a
    /// state reset.
    pub(super) ingest_dispatcher_slot:
        &'a Arc<std::sync::RwLock<crate::substrate::EventIngestDispatcher>>,
    /// V-40 — shared [`crate::substrate::DmInboxRelayLookup`] slot. Same
    /// `Reset`-survival contract as the ingest dispatcher slot.
    pub(super) dm_inbox_relays_slot: &'a Arc<Mutex<Arc<dyn crate::substrate::DmInboxRelayLookup>>>,
    /// Shared [`crate::substrate::BlockedRelayLookup`] slot. Same
    /// `Reset`-survival contract as `dm_inbox_relays_slot`.
    pub(super) blocked_relays_slot: &'a Arc<Mutex<Arc<dyn crate::substrate::BlockedRelayLookup>>>,
    /// Per-app bootstrap Tailing self-kinds override slot. `None` →
    /// kernel reverts to its built-in default. Read by the `Reset` arm
    /// to re-apply the override against the rebuilt kernel.
    pub(super) bootstrap_self_kinds_slot: &'a Arc<Mutex<Option<Vec<u64>>>>,
    /// V-51 phase 4 — routing-trace projection slot. Read by the `Reset`
    /// arm to re-publish the rebuilt kernel's `routing_trace()` clone so
    /// `NmpApp::routing_trace` keeps returning a live projection across a
    /// state wipe.
    pub(super) routing_trace_slot:
        &'a Arc<Mutex<Option<Arc<crate::kernel::routing_trace::RoutingTraceProjection>>>>,
    /// V-83 — event-store publish-back slot. Read by the `Reset` arm to
    /// re-publish the rebuilt kernel's `event_store_handle()` clone (the rebuild
    /// constructs a fresh `EventStore`) so `NmpApp::event_by_id` keeps reading
    /// the live store across a state wipe — same publish-back-on-`Reset`
    /// contract as `routing_trace_slot`.
    pub(super) event_store_slot: &'a crate::slots::EventStoreSlot,
    /// V-51 phase 5 — per-app substrate-routing factory slot. Re-invoked by
    /// the `Reset` arm against the rebuilt kernel's fresh projection clone
    /// so a production router (e.g. `nmp_router::GenericOutboxRouter`)
    /// survives a state wipe — same contract as the ingest dispatcher /
    /// dm-inbox-lookup / routing-trace slots above.
    pub(super) routing_substrate_slot: &'a crate::slots::RoutingSubstrateSlot,
    /// Spec §271 (2026-05-25) — same contract as `routing_substrate_slot`,
    /// for the publish-side resolver. Re-applied by the `Reset` arm against
    /// the rebuilt kernel's fresh handles so the production
    /// `nmp_router::Nip65OutboxResolver` survives a state wipe.
    pub(super) publish_resolver_slot: &'a crate::slots::PublishResolverSlot,
    /// V-82 — the FFI-shared active-account hex-pubkey slot. The kernel writes
    /// its active account into this `Arc` on every identity mutation and the
    /// host (`nmp-ffi::NmpApp::active_account_handle`) holds the same `Arc`.
    /// Read by the `Reset` arm to rebuild the kernel through
    /// [`crate::kernel::Kernel::with_storage_path_and_account_slot`] with the
    /// same slot, so the shared handle survives a state wipe — same contract
    /// as the routing-trace projection re-publish above.
    pub(super) active_account_slot: &'a crate::slots::ActiveAccountSlot,
    /// Raw-event forwarding observer ids. Policies receive kernel handles,
    /// so `Reset` unregisters observers pinned to the discarded kernel and
    /// re-registers them against fresh handles.
    pub(super) raw_event_forward_observer_ids:
        &'a crate::actor::raw_event_forwarder::RawEventForwardObserverIdSlot,
    /// Policy factory slot used when registering raw-event forwarders.
    pub(super) raw_event_forward_policy_slot: &'a crate::slots::RawEventForwardPolicySlot,
    /// Shared raw-event tap slot — held in the actor scope and threaded
    /// through here so the `Reset` arm can re-register the pipeline
    /// observer against the same `RawEventObserverSlot` (which itself
    /// survives Reset via `take_raw_event_observers_handle_for_reset`).
    pub(super) raw_event_observers_handle: &'a crate::actor::RawEventObserverSlot,
}

// ────────────────────────────────────────────────────────────────────────
// Debt C — capability adapters for `ProtocolCommandContext`.
//
// The `Protocol(cmd)` arm constructs these to bridge the actor's
// kernel + identity references into the typed capability traits the
// substrate `ProtocolCommandContext` consumes. Lifetimes are bound to
// the dispatch arm's stack frame; the adapters never outlive their
// `RefCell` borrow targets.
// ────────────────────────────────────────────────────────────────────────

struct KernelClockAdapter<'a> {
    kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

// SAFETY: the dispatch arm constructs and drops the adapter on the
// actor thread; the `&RefCell<&mut Kernel>` reference never crosses a
// thread boundary. The `Send + Sync` claim is needed because the
// substrate trait carries the bound (`dyn KernelClock` lives behind
// `&dyn` in `ProtocolCommandContext`), but the adapter is held only
// for the dispatch arm's stack frame.
unsafe impl<'a> Send for KernelClockAdapter<'a> {}
unsafe impl<'a> Sync for KernelClockAdapter<'a> {}

impl<'a> crate::substrate::KernelClock for KernelClockAdapter<'a> {
    fn now_secs(&self) -> u64 {
        self.kernel.borrow().now_secs()
    }
}

struct LocalSignerAccessAdapter<'a> {
    identity: &'a std::cell::RefCell<&'a IdentityRuntime>,
}

unsafe impl<'a> Send for LocalSignerAccessAdapter<'a> {}
unsafe impl<'a> Sync for LocalSignerAccessAdapter<'a> {}

impl<'a> crate::substrate::LocalSignerAccess for LocalSignerAccessAdapter<'a> {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        self.identity.borrow().active_local_keys().cloned()
    }
    fn signer_for_seal(&self) -> Option<Arc<dyn nmp_nip59::SignerForSeal>> {
        self.identity.borrow().active_signer_for_seal()
    }
}

struct ErrorSurfaceAdapter<'a> {
    kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for ErrorSurfaceAdapter<'a> {}
unsafe impl<'a> Sync for ErrorSurfaceAdapter<'a> {}

impl<'a> crate::substrate::ErrorSurface for ErrorSurfaceAdapter<'a> {
    fn set_last_error_toast(&self, message: Option<String>) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.set_last_error_toast(message);
        }
    }
    fn record_action_failure(&self, correlation_id: String, reason: String) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.record_action_failure(correlation_id, reason);
        }
    }
}

struct ActionStageTrackerAdapter<'a> {
    kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for ActionStageTrackerAdapter<'a> {}
unsafe impl<'a> Sync for ActionStageTrackerAdapter<'a> {}

impl<'a> crate::substrate::ActionStageTracker for ActionStageTrackerAdapter<'a> {
    fn record_requested(&self, correlation_id: &str) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.record_action_stage(
                correlation_id,
                crate::kernel::action_stages::ActionStage::Requested,
                None,
            );
        }
    }
}

/// Debt-C-follow-up — bridge the kernel's `outbox_router` slot into the
/// substrate [`crate::substrate::RecipientRelayLookup`] capability. NIP-57
/// LNURL fetcher consumes this to populate the kind:9734 `relays` tag
/// (recipient's NIP-65 write set + cold-start fallback) without naming
/// `OutboxRouter` or the substrate `MailboxCache` directly.
struct RecipientRelayLookupAdapter<'a> {
    kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for RecipientRelayLookupAdapter<'a> {}
unsafe impl<'a> Sync for RecipientRelayLookupAdapter<'a> {}

impl<'a> crate::substrate::RecipientRelayLookup for RecipientRelayLookupAdapter<'a> {
    fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        // Kernel read; no mutation required. `try_borrow` keeps the
        // adapter total in the presence of a re-entrant kernel borrow on
        // the dispatch arm (defensive — production has no such cycle).
        self.kernel
            .try_borrow()
            .ok()
            .map(|k| k.recipient_publish_relays(recipient, kind))
            .unwrap_or_default()
    }
}

/// M2 (ADR-0042) — build the `(SubIdentity, LogicalInterest)` pair for an
/// `OpenInterest` / `CloseInterest` command from the raw FFI arguments.
///
/// Shared by both arms so an open and its matching close land on the SAME
/// registry `(scope, key)` slot: the `SubKey` is the hash of the parsed
/// `InterestShape` (order-independent — see `InterestShape::from_filter_json`),
/// the `SubOwnerKey` is the hash of `consumer_id`, and the `SubScope` folds
/// `ActiveAccount` → `Global` (the registry's existing `legacy_scope`
/// convention — the registry's `SubScope` has no `ActiveAccount` variant, so
/// the real `InterestScope::ActiveAccount` rides on the `LogicalInterest`
/// instead, where the compiler reads it to re-route on account switch).
///
/// Returns `None` when `filter_json` is not a valid NIP-01 filter object (D6 —
/// the arm treats it as a silent no-op).
fn build_open_interest(
    filter_json: &str,
    consumer_id: &str,
    scope: u32,
) -> Option<(crate::subs::SubIdentity, crate::planner::LogicalInterest)> {
    use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
    use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

    let shape = crate::planner::InterestShape::from_filter_json(filter_json)?;

    // `0` = ActiveAccount (re-route on switch), anything else = Global.
    let interest_scope = if scope == 0 {
        InterestScope::ActiveAccount
    } else {
        InterestScope::Global
    };

    // Registry key: the SubScope mirrors `InterestRegistry::legacy_scope`
    // (ActiveAccount shares the Global slot space until per-account isolation
    // resolves the active pubkey). The real account-context lives on the
    // LogicalInterest below.
    let sub_scope = SubScope::Global;
    // Fold the scope discriminant into the key so an ActiveAccount and a Global
    // open of the *same* filter never collide on one slot (they route
    // differently).
    let key = SubKey::builder("open-interest")
        .with(&shape)
        .with(scope)
        .finish();
    let identity = SubIdentity::new(SubOwnerKey::new(consumer_id), key, sub_scope);

    let interest = LogicalInterest {
        scope: interest_scope,
        shape,
        // `open_interest` is always a tailing feed subscription (never OneShot).
        lifecycle: InterestLifecycle::Tailing,
        ..LogicalInterest::default()
    };

    Some((identity, interest))
}

pub(super) fn dispatch_command(
    command: ActorCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match command {
        ActorCommand::Start {
            visible_limit,
            emit_hz: hz,
            initial_relays,
        } => {
            *ctx.running = true;
            *ctx.emit_hz = hz;
            *ctx.startup_sent = false;
            ctx.kernel.set_visible_limit(visible_limit);
            // Seed the app-declared initial relay configuration into
            // `configured_relays` before the session restore runs. There is no
            // hardcoded default: an app with no declared relays (and no pre-start
            // `add_relay`) starts with an empty set and the kernel surfaces the
            // `no_configured_relays` diagnostic (V-66) rather than silently
            // dialing an unconsented relay.
            if !initial_relays.is_empty() {
                let rows: Vec<crate::kernel::AppRelay> = initial_relays
                    .iter()
                    .filter_map(|(url, role)| {
                        let url = crate::relay::canonical_relay_url(url)?;
                        let role = crate::actor::canonical_relay_role(role)?;
                        Some(crate::kernel::AppRelay::new(url, role))
                    })
                    .collect();
                if !rows.is_empty() {
                    ctx.kernel.set_configured_relays(rows);
                }
            }
            ctx.kernel.start();
            // ADR-0040 §3: restore_active_session stays synchronous (cold-start
            // read chain; see session_persistence.rs module doc). The tail
            // writes (persist_current_active_session) are enqueued off-actor.
            let mut outbound = session_persistence::restore_active_session(
                ctx.identity,
                ctx.kernel,
                ctx.capability_callback,
                ctx.capability_work_tx,
                ctx.relays_ready,
            );
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
            // D1 — first snapshot must reach the shell before any relay TCP
            // connection is dialed, so emit_now precedes spawn_missing_relays.
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            spawn_missing_relays(
                ctx.relay_controls,
                ctx.slot_to_url,
                ctx.pool,
                ctx.kernel,
                ctx.next_relay_generation,
            );
            // T127: boot-resume for the publish engine. Closes Residual 3
            // from T117 — `accepted_locally` rows persisted by a previous
            // process come back as `InFlight` and any due retries dispatch
            // immediately. Today the production store is fresh in-memory
            // per process so this is a no-op; once the M3 LMDB store lands
            // the resume call will drive the resurrected rows back through
            // the actor's normal outbound path. `spawn_missing_relays`
            // above ran first, so workers will spawn on demand for any
            // URL the resumed frames target (idempotent via
            // `ensure_relay_worker`). Frames flow through the regular
            // `send_all_outbound` call in `run_actor`.
            outbound.extend(ctx.kernel.resume_publish_engine());
            Some(outbound)
        }
        ActorCommand::Configure {
            visible_limit,
            emit_hz: hz,
        } => {
            *ctx.emit_hz = hz;
            ctx.kernel.set_visible_limit(visible_limit);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::OpenAuthor { pubkey, kinds } => {
            let outbound = ctx.kernel.open_author(pubkey, kinds, ctx.relays_ready);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::OpenThread { event_id, kinds } => {
            let outbound = ctx.kernel.open_thread(event_id, kinds, ctx.relays_ready);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::ClaimProfile {
            pubkey,
            consumer_id,
            force,
        } => {
            let outbound = ctx
                .kernel
                .claim_profile(pubkey, consumer_id, ctx.relays_ready, force);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::ReleaseProfile {
            pubkey,
            consumer_id,
        } => {
            let outbound = ctx.kernel.release_profile(&pubkey, &consumer_id);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::ClaimEvent {
            uri,
            consumer_id,
            force,
        } => {
            let outbound = ctx
                .kernel
                .claim_event(uri, consumer_id, ctx.relays_ready, force);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::ReleaseEvent { uri, consumer_id } => {
            let outbound = ctx.kernel.release_event(&uri, &consumer_id);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::CloseAuthor { pubkey } => {
            let outbound = ctx.kernel.close_author(&pubkey);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::CloseThread { event_id } => {
            let outbound = ctx.kernel.close_thread(&event_id);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::SignEventForReturn {
            account_pubkey,
            unsigned_json,
            correlation_id,
        } => {
            // D13 sign-and-return: sign the host's draft with the named (or
            // active) account and hand the signed JSON straight back through
            // the `signed_events` projection — NEVER publish. Closes the gap
            // where a host needed raw private key bytes to sign a Blossom /
            // feedback auth event, which is impossible for NIP-46 bunker users.
            //
            // The host draft is `{ kind, content, tags, created_at? }` — it
            // carries no `pubkey` (the host does not know which signer will be
            // used) and its `created_at` is advisory. Parse the partial draft
            // and fill `pubkey` from the resolved account + re-stamp
            // `created_at` from the kernel clock (D7 — the host never owns
            // wall-clock time).
            let signer_pubkey = if account_pubkey.is_empty() {
                ctx.identity.active_pubkey()
            } else {
                Some(account_pubkey.clone())
            };
            let Some(signer_pubkey) = signer_pubkey else {
                ctx.kernel.record_signed_event_return(
                    &correlation_id,
                    Err("no active account — sign in first".to_string()),
                );
                maybe_emit_after_dispatch(
                    ctx.kernel,
                    *ctx.running,
                    ctx.update_tx,
                    ctx.last_emit,
                );
                return Some(Vec::new());
            };
            let unsigned = match build_unsigned_for_return(
                &unsigned_json,
                &signer_pubkey,
                ctx.kernel.now_secs(),
            ) {
                Ok(unsigned) => unsigned,
                Err(reason) => {
                    ctx.kernel.record_signed_event_return(
                        &correlation_id,
                        Err(format!("invalid unsigned_json: {reason}")),
                    );
                    maybe_emit_after_dispatch(
                        ctx.kernel,
                        *ctx.running,
                        ctx.update_tx,
                        ctx.last_emit,
                    );
                    return Some(Vec::new());
                }
            };
            // Non-blocking sign (D8): a local key resolves on the spot; a
            // NIP-46 bunker returns `Pending` and is parked below.
            let sign_result = if account_pubkey.is_empty() {
                commands::sign_active_nonblocking(ctx.identity, &unsigned)
            } else {
                commands::sign_with_account_nonblocking(ctx.identity, &signer_pubkey, &unsigned)
            };
            match sign_result {
                Err(reason) => {
                    ctx.kernel
                        .record_signed_event_return(&correlation_id, Err(reason));
                }
                Ok(mut op) => match op.poll() {
                    Some(Ok(signed)) => {
                        ctx.kernel.record_signed_event_return(
                            &correlation_id,
                            Ok(signed_event_to_json(&signed)),
                        );
                    }
                    Some(Err(e)) => {
                        ctx.kernel
                            .record_signed_event_return(&correlation_id, Err(e.to_string()));
                    }
                    None => {
                        // Remote (NIP-46) signer parked — resolved by the idle
                        // loop into the `signed_events` projection. `op` was
                        // moved out of `sign_result`, so reconstruct the park
                        // from the (unconsumed) `Pending` op.
                        ctx.pending_sign_returns
                            .push(PendingSignReturn::new(op, correlation_id.clone()));
                    }
                },
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::SignEventForAccount {
            unsigned,
            signer_pubkey,
            continuation,
        } => {
            // ADR-0043 Decision 2 — generic, backend-transparent sign-account
            // port. Sign with the active account (`signer_pubkey == None`) or a
            // named roster key, then deliver the resolved `SignedEvent` (or an
            // error string) to the boxed continuation. Local-vs-bunker is
            // invisible: both resolve through the same nonblocking sign + park
            // machinery (D8). The continuation is the sole consumer of the
            // signed event — `nmp-core` never parses it and learns no protocol
            // noun (D0). D13: only a `SignedEvent` ever crosses to the
            // continuation, never raw key bytes.
            let sign_result = match &signer_pubkey {
                None => commands::sign_active_nonblocking(ctx.identity, &unsigned),
                Some(pk) => commands::sign_with_account_nonblocking(ctx.identity, pk, &unsigned),
            };
            match sign_result {
                Err(reason) => {
                    // No active/named account, or no signer for the pubkey:
                    // resolve the continuation with the error so the worker's
                    // failure path runs and the host spinner clears (D6).
                    continuation.call(Err(reason));
                }
                Ok(mut op) => match op.poll() {
                    Some(Ok(signed)) => {
                        // Local key — `Ready` resolves on the spot. Invoke the
                        // continuation inline on the actor thread.
                        continuation.call(Ok(signed));
                    }
                    Some(Err(e)) => {
                        continuation.call(Err(e.to_string()));
                    }
                    None => {
                        // Remote (NIP-46) signer — `Pending`. Park with the
                        // continuation sink; the idle-loop drain invokes the
                        // continuation when the broker turns the request around
                        // (or on timeout, with an `Err`). `op` was moved out of
                        // `sign_result`, so reconstruct the park from the
                        // unconsumed `Pending` op.
                        ctx.pending_sign_returns
                            .push(PendingSignReturn::with_continuation(op, continuation));
                    }
                },
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::AddSigner {
            source,
            make_active,
        } => {
            use crate::actor::SignerSource;
            // A `BunkerUri` source kicks off an async handshake and stores no
            // signer yet — no keyring persistence runs until the resolved
            // `RemoteHandle` arrives. A `RemoteHandle` source must persist the
            // remote-signer payload off-actor; capture its persistence metadata
            // BEFORE `add_signer` consumes the `source`.
            let is_bunker_handshake = matches!(source, SignerSource::BunkerUri(_));
            let remote_persistence = match &source {
                SignerSource::RemoteHandle(handle) => {
                    Some((handle.pubkey_hex(), handle.persistence_payload_json()))
                }
                _ => None,
            };
            let outbound = commands::add_signer(
                ctx.identity,
                ctx.kernel,
                source,
                make_active,
                ctx.relays_ready,
            );
            // ADR-0040 §3 — enqueue all Keychain writes off-actor (D8). The
            // bunker-handshake-initiation path has nothing to persist yet (the
            // signer arrives later as a `RemoteHandle`); skip persistence so we
            // don't write a session for an account that doesn't exist yet.
            if !is_bunker_handshake {
                if let Some((remote_identity_id, Some(payload_json))) = &remote_persistence {
                    session_persistence::enqueue_persist_remote_signer_payload(
                        remote_identity_id,
                        payload_json,
                        ctx.capability_work_tx,
                    );
                }
                update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
                session_persistence::enqueue_persist_current_active_session(
                    ctx.identity,
                    ctx.capability_work_tx,
                );
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::CreateAccount {
            profile,
            relays,
            mls,
            make_active,
        } => {
            let outbound = commands::create_account(
                ctx.identity,
                ctx.kernel,
                ctx.relays_ready,
                &profile,
                &relays,
                mls,
                make_active,
            );
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
            // ADR-0040 §3 — enqueue the Keychain write off-actor (D8).
            session_persistence::enqueue_persist_current_active_session(
                ctx.identity,
                ctx.capability_work_tx,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::SwitchActive { identity_id } => {
            let outbound =
                commands::switch_active(ctx.identity, ctx.kernel, &identity_id, ctx.relays_ready);
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
            // ADR-0040 §3 — enqueue the Keychain write off-actor (D8).
            session_persistence::enqueue_persist_current_active_session(
                ctx.identity,
                ctx.capability_work_tx,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::RemoveAccount { identity_id } => {
            let outbound = commands::remove_account(ctx.identity, ctx.kernel, &identity_id);
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
            // ADR-0040 §3 — enqueue the Keychain forget + active-pointer
            // persist off-actor (D8). FIFO ordering ensures forget(acct-X)
            // executes before any subsequent persist for the new active
            // account — the single worker drains in enqueue order.
            session_persistence::enqueue_forget_account(&identity_id, ctx.capability_work_tx);
            session_persistence::enqueue_persist_current_active_session(
                ctx.identity,
                ctx.capability_work_tx,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::BunkerHandshakeProgress { stage, message } => {
            commands::bunker_handshake_progress(ctx.identity, ctx.kernel, stage, message);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::BunkerConnectionStateChanged { state, reason } => {
            commands::bunker_connection_state_changed(ctx.identity, ctx.kernel, state, reason);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::PublishRawEvent {
            kind,
            tags,
            content,
            target,
            signer_pubkey,
            correlation_id,
        } => {
            // D7: kernel owns the wall clock. Unlike `PublishUnsignedEvent`
            // below — whose callers (NIP-crate executors) set the sentinel
            // `created_at: 0` and rely on the dispatch arm to stamp — this
            // arm builds the `UnsignedEvent` itself, so we stamp inline
            // from `kernel.now_secs()` directly. Same effect, no sentinel
            // round-trip required. The FixedClock test hook plugs into
            // `kernel.now_secs()`, so end-to-end behaviour is preserved.
            //
            // `pubkey` is intentionally left empty: both
            // `publish_unsigned_event` and `publish_unsigned_event_to_relays`
            // ignore the caller's `unsigned.pubkey` and write the active
            // identity's pubkey onto the SignedEvent at sign time. Setting
            // it here would be dead work.
            let unsigned = crate::substrate::UnsignedEvent {
                pubkey: String::new(),
                kind,
                tags,
                content,
                created_at: ctx.kernel.now_secs(),
            };
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            // Route on `target`: `Auto` resolves via NIP-65 outbox (D3);
            // `Explicit { relays }` pins to exactly those relays. Both
            // helpers handle local-keys (sync sign) and bunker (parked
            // PendingSign) paths internally — `PublishRaw` inherits the
            // same identity-kind support as `PublishProfile`.
            let outbound = match target {
                crate::publish::PublishTarget::Auto => commands::publish_unsigned_event(
                    ctx.identity,
                    ctx.kernel,
                    unsigned,
                    correlation_id,
                    // Honour the `PublishRaw` signer selector: `None` signs with
                    // the active account; `Some(pubkey)` signs with that
                    // registered agent / per-podcast key (app-signer-slot.md).
                    signer_pubkey,
                    ctx.pending_signs,
                ),
                crate::publish::PublishTarget::Explicit { relays } => {
                    commands::publish_unsigned_event_to_relays(
                        ctx.identity,
                        ctx.kernel,
                        unsigned,
                        relays,
                        correlation_id,
                        // Honour the `PublishRaw` signer selector: `None` signs
                        // with the active account; `Some(pubkey)` signs with that
                        // registered agent / per-podcast key (app-signer-slot.md).
                        signer_pubkey,
                        ctx.pending_signs,
                    )
                }
            };
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishProfile {
            fields,
            correlation_id,
        } => {
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::publish_profile(
                ctx.identity,
                ctx.kernel,
                fields,
                correlation_id,
                ctx.pending_signs,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishUnsignedEvent {
            event: mut unsigned,
            correlation_id,
            signer_pubkey,
        } => {
            // D7: apply the same created_at=0 sentinel as PublishUnsignedEventToRelays.
            // A host that builds an UnsignedEvent without setting created_at gets
            // the kernel clock rather than epoch time.
            if unsigned.created_at == 0 {
                unsigned.created_at = ctx.kernel.now_secs();
            }
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::publish_unsigned_event(
                ctx.identity,
                ctx.kernel,
                unsigned,
                correlation_id,
                signer_pubkey,
                ctx.pending_signs,
            );
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishUnsignedEventToRelays {
            mut event,
            relays,
            correlation_id,
            signer_pubkey,
        } => {
            // D7: kernel owns the wall clock. Executors in NIP crates set
            // created_at = 0 as a sentinel; we re-stamp here so they never
            // call SystemTime::now() and the FixedClock test hook stays
            // effective end-to-end.
            if event.created_at == 0 {
                event.created_at = ctx.kernel.now_secs();
            }
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::publish_unsigned_event_to_relays(
                ctx.identity,
                ctx.kernel,
                event,
                relays,
                correlation_id,
                signer_pubkey,
                ctx.pending_signs,
            );
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishSignedEvent {
            raw,
            target,
            correlation_id,
        } => {
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::publish_signed_event(ctx.kernel, raw, target, correlation_id);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        // V-39: `ActorCommand::SendGiftWrappedDm` arm deleted — the
        // equivalent flow now dispatches `ActorCommand::Protocol(Box::new(
        // nmp_nip17::SendGiftWrappedDmCommand { ... }))`. The protocol-
        // command body runs in the `ActorCommand::Protocol` arm below; it
        // reaches the active local keys, the DM-inbox cache, and the
        // publish engine through the substrate `ProtocolCommandContext`.
        ActorCommand::RetryPublish { handle } => {
            let outbound = ctx.kernel.retry_publish_now(&handle);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::CancelPublish { handle } => {
            ctx.kernel.cancel_publish(&handle);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::React {
            target_event_id,
            reaction,
            correlation_id,
        } => {
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::react(
                ctx.identity,
                ctx.kernel,
                &target_event_id,
                &reaction,
                correlation_id,
                ctx.pending_signs,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::Follow {
            pubkey,
            correlation_id,
        } => {
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::follow(
                ctx.identity,
                ctx.kernel,
                &pubkey,
                true,
                correlation_id,
                ctx.pending_signs,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::Unfollow {
            pubkey,
            correlation_id,
        } => {
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::follow(
                ctx.identity,
                ctx.kernel,
                &pubkey,
                false,
                correlation_id,
                ctx.pending_signs,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::AddRelay { url, role } => {
            // T158: add_relay now returns Some(canonical_url) on success so we
            // can dial a real socket immediately. User-added relays use
            // RelayRole::Content as the diagnostic lane (inbox/outbox bucket);
            // the NIP-65 read/write distinction lives in AppRelay, not in
            // the transport pool key (T105). ensure_relay_worker is idempotent —
            // a role-edit for an already-connected URL is a harmless no-op.
            //
            // T-nip65-auto-publish: snapshot the projection BEFORE the mutation
            // so we can compare-and-skip the re-publish when the call was a
            // pure no-op (re-adding the same URL with the same role). Without
            // this every harmless re-add re-published kind:10002 and burned a
            // relay write.
            let projection_before = ctx.kernel.configured_relays_snapshot().to_vec();
            let mut outbound = Vec::new();
            if let Some(canonical_url) = commands::add_relay(ctx.kernel, &url, &role) {
                ensure_relay_worker(
                    ctx.relay_controls,
                    ctx.slot_to_url,
                    ctx.pool,
                    ctx.kernel,
                    ctx.next_relay_generation,
                    crate::relay::RelayRole::Content,
                    canonical_url,
                );
                outbound.extend(maybe_publish_relay_list_after_edit(
                    ctx.identity,
                    ctx.kernel,
                    &projection_before,
                    ctx.pending_signs,
                ));
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::RemoveRelay { url } => {
            // T162 + T-relay-url-normalize: both shutdown_relay_worker and
            // commands::remove_relay canonicalize the URL internally (lowercase
            // scheme+host, strip empty-path trailing slash) so that the pool key
            // and AppRelay.url always agree regardless of how the FFI caller
            // spelled the URL. Shutdown the worker first so the socket is closed
            // before the projection row is removed. Idempotent: if no worker exists
            // for the URL, shutdown_relay_worker returns false and the projection
            // mutation still proceeds normally (D6: no silent drops).
            //
            // T-nip65-auto-publish: same compare-and-skip as `AddRelay` above.
            // Removing a URL that was never present is a no-op and must NOT
            // re-publish kind:10002.
            let projection_before = ctx.kernel.configured_relays_snapshot().to_vec();
            shutdown_relay_worker(ctx.relay_controls, ctx.slot_to_url, ctx.pool, &url);
            commands::remove_relay(ctx.kernel, &url);
            let outbound = maybe_publish_relay_list_after_edit(
                ctx.identity,
                ctx.kernel,
                &projection_before,
                ctx.pending_signs,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::OpenContactListSubscription { kinds } => {
            let outbound =
                commands::open_contact_list_sub(ctx.identity, ctx.kernel, ctx.relays_ready, kinds);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        // V-38: `ActorCommand::Wallet{Connect,Disconnect,PayInvoice}`
        // variants were deleted. Wallet ops now route through
        // `ActorCommand::Protocol(Box<dyn ProtocolCommand>)` — the
        // `WalletConnectCommand` / `WalletDisconnectCommand` /
        // `WalletPayInvoiceCommand` impls live in `crates/nmp-nip47`.
        //
        // V-41 — the legacy `FetchLnurlInvoice` arm is also deleted. The LNURL
        // fetcher now lives in `nmp_nip57::lnurl::FetchLnurlInvoiceCommand`
        // and dispatches through `ActorCommand::Protocol` (below). The
        // pre-existing `Requested` stage recording (gated on
        // `correlation_id`) and the post-dispatch `emit_now` both moved
        // into the `Protocol(...)` arm — see
        // `ProtocolCommandContext::record_action_stage_requested` and the
        // emit at the bottom of that arm.
        ActorCommand::RecordActionFailure {
            correlation_id,
            reason,
        } => {
            // Writes `Failed { reason }` to `action_stages` and a terminal
            // verdict to `action_results` — both surfaces the host uses to
            // clear the spinner. Without this, an executor that fails before
            // emitting an ActorCommand would orphan the correlation_id.
            ctx.kernel.record_action_failure(correlation_id, reason);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::RecordActionSuccess {
            correlation_id,
            result_json,
        } => {
            // Symmetric counterpart to RecordActionFailure: off-thread workers
            // and runtime responders fan success back through the actor
            // channel. Writes `Accepted` to `action_stages` and a terminal
            // verdict to `action_results`. `result_json` (ADR-0043 Decision 4)
            // rides into the `action_results` row's `result` field verbatim.
            ctx.kernel.record_action_success(correlation_id, result_json);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::AckActionStage(correlation_id) => {
            ctx.kernel.ack_action_stage(&correlation_id);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::LifecycleEvent(phase) => {
            // T118 / G3 — fold scenePhase into the kernel state and fire
            // the registered observer (if any) on a meaningful transition.
            // The handler is idempotent (rapid scene oscillation collapses
            // to a single observer call) and never emits outbound frames;
            // the consumer's TriggerEngine drives any reconcile work
            // through its own path on the next tick.
            commands::handle_lifecycle_event(ctx.kernel, ctx.lifecycle_observer, phase);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::Kernel(action) => {
            // The kernel action mutates state; the next periodic snapshot
            // emission carries any visible effect (e.g. registered interests).
            // The discrete `{"t":"update","v":…}` frame channel was deleted as
            // shipped-but-inert — every host bridge only consumed snapshots.
            let _ = dispatch_kernel_action(ctx.kernel, action);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::ShowToast { message } => {
            // D6 — FFI-boundary validation errors reach the kernel as state
            // via this command. The FFI layer only has a channel sender; this
            // arm is the single path from the FFI to `set_last_error_toast`.
            ctx.kernel.set_last_error_toast(Some(message));
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::MarkChangedSinceEmit => {
            ctx.kernel.mark_changed_since_emit();
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::DispatchHostOp {
            action_json,
            correlation_id,
        } => {
            // Substrate-generic seam for stateful, app-owned op handlers
            // (today: `nmp-app-marmot`'s MLS service). The handler was installed
            // via `NmpApp::set_host_op_handler` during host init.
            //
            // Record `Requested` first so the host's spinner sees the action
            // entered the actor lane even if the handler is absent or panics
            // (mirrors the `WalletPayInvoice` arm and the V-41 LNURL
            // protocol command — see
            // `nmp_nip57::lnurl::FetchLnurlInvoiceCommand`).
            ctx.kernel.record_action_stage(
                &correlation_id,
                crate::kernel::action_stages::ActionStage::Requested,
                None,
            );
            // Pull the handler clone OUT of the slot before calling `handle`
            // so the outer mutex is not held across the SQLite-bound work
            // (D8 — long-running ops must not block the slot writer).
            let handler = ctx
                .host_op_handler
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().cloned());
            let result = match handler {
                Some(handler) => {
                    // D6 — wrap in catch_unwind so a buggy handler that panics
                    // does NOT unwind across the FFI boundary; mirror
                    // `ActionRegistry::execute`'s pattern.
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        handler.handle(&action_json, &correlation_id)
                    }))
                    .unwrap_or_else(|_| {
                        serde_json::json!({
                            "ok": false,
                            "error": "host op handler panicked"
                        })
                    })
                }
                None => serde_json::json!({
                    "ok": false,
                    "error": "no host op handler installed"
                }),
            };
            // Route the envelope to the action_results/action_stages mirror.
            // Convention (matches the rest of the substrate dispatch ops):
            // `{"ok": true, ...}` → success; anything else → failure with the
            // `error` field as the reason (defaulting to a static string when
            // missing so the host always sees something renderable).
            let ok = result
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if ok {
                // Host-op success carries no structured result body (D0).
                ctx.kernel.record_action_success(correlation_id, None);
            } else {
                let reason = result
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("host op failed without an error message")
                    .to_string();
                ctx.kernel.record_action_failure(correlation_id, reason);
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        #[cfg(feature = "native")]
        ActorCommand::CapabilityResultReady {
            account_id,
            result_json,
        } => {
            // ADR-0040 §3 — re-entry from the serialized capability-worker.
            //
            // The worker ran `dispatch_capability` off the actor thread and
            // posted this command. We apply the result here, inside a normal
            // actor tick (D4 — actor sole writer).
            //
            // Account-switch safety: if the account was removed or switched
            // away between enqueue and now, drop the result with a D6 trace
            // (never cross-apply to the now-active account). This is the
            // architectural guarantee that makes the single FIFO worker
            // correct: forget(A) followed by a switch cannot misapply a
            // stale persist(A) result to account B.
            if !ctx.identity.contains_account(&account_id) {
                // D6 — removed-account result is data (a trace), not an error.
                // No toast: the account was deliberately removed; the user
                // doesn't need to know the Keychain write was pre-empted.
                tracing::trace!(
                    "CapabilityResultReady: dropped result for removed account {account_id}"
                );
                return Some(Vec::new());
            }
            // Decode the outer CapabilityEnvelope and check the inner
            // KeyringResult status. An error result surfaces a D6 toast so
            // the user sees "keychain write failed" rather than a silent
            // secret-not-persisted bug. Success results are no-ops (the
            // write is already done on the Keychain).
            let decoded =
                serde_json::from_str::<crate::substrate::CapabilityEnvelope>(&result_json)
                    .ok()
                    .map(|env| crate::substrate::KeyringIdentityWiring::decode_result(&env));
            if let Some(result) = decoded {
                use crate::substrate::KeyringStatus;
                match result.status {
                    KeyringStatus::Ok => {
                        // Write succeeded — no observable actor-state change needed.
                    }
                    KeyringStatus::NotFound | KeyringStatus::Error => {
                        // D6 — surface as a toast so the user can see the
                        // Keychain write failed (session may not persist).
                        ctx.kernel.set_last_error_toast(Some(format!(
                            "keyring write failed for account {account_id}: {:?}",
                            result.status
                        )));
                        maybe_emit_after_dispatch(
                            ctx.kernel,
                            *ctx.running,
                            ctx.update_tx,
                            ctx.last_emit,
                        );
                    }
                }
            }
            Some(Vec::new())
        }
        ActorCommand::Stop => {
            *ctx.running = false;
            *ctx.startup_sent = false;
            close_relays(
                ctx.relay_controls,
                ctx.slot_to_url,
                ctx.pool,
                ctx.connected_relays,
                ctx.kernel,
            );
            // T116/G1 — clear reconnect-replay discriminator so a subsequent
            // Start replays cleanly (every URL appears as a first-connect).
            ctx.connected_urls.clear();
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::Reset => {
            close_relays(
                ctx.relay_controls,
                ctx.slot_to_url,
                ctx.pool,
                ctx.connected_relays,
                ctx.kernel,
            );
            ctx.connected_urls.clear();
            // T114b — preserve the FFI-channel drop-counter handle across
            // Reset (the underlying Arc<AtomicU64> is shared with the FFI
            // forwarder thread and must NOT be replaced; the counter is
            // process-lifetime).
            let drops_handle = ctx.kernel.take_dispatch_drops_handle_for_reset();
            // G-S4 — preserve the actor command-channel depth counter across
            // Reset for the same reason: the `Arc<AtomicU64>` is shared with
            // `NmpApp::send_cmd`; replacing it would orphan the counter so
            // every subsequent send increments into a handle the kernel no
            // longer reads.
            let queue_depth_handle = ctx.kernel.take_queue_depth_handle_for_reset();
            // T146 — preserve the event observer slot across Reset for the
            // same reason: the `Arc<Mutex<…>>` is shared with the FFI
            // surface and per-app crates; replacing it would silently
            // disconnect every registered observer.
            let event_observers_handle = ctx.kernel.take_event_observers_handle_for_reset();
            // Preserve the raw signed-event tap slot across Reset for the
            // same reason: the `Arc<Mutex<…>>` is shared with the FFI
            // surface and per-app crates; replacing it would silently
            // disconnect every registered raw observer.
            let raw_event_observers_handle = ctx.kernel.take_raw_event_observers_handle_for_reset();
            // Preserve the snapshot-projection slot across Reset for the same
            // reason: the `Arc<Mutex<…>>` is shared with the FFI surface and
            // per-app crates; replacing it would silently drop every
            // host-registered projection from the snapshot.
            let snapshot_projection_handle = ctx.kernel.take_snapshot_projection_handle_for_reset();
            // Preserve the relay-edit rows handle across Reset for the same
            // reason: the `Arc<Mutex<…>>` is shared with the FFI surface
            // and per-app crates; replacing it would silently return stale
            // rows to the host-app dispatch layer.
            let configured_relays_handle = ctx.kernel.take_app_relay_slot_for_reset();
            // NOTE: the FFI-supplied LMDB `storage_path` (from
            // `nmp_app_set_storage_path`) is NOT re-threaded here — `Reset`
            // rebuilds the kernel with the in-memory store unless the
            // `NMP_LMDB_PATH` env-var fallback in `build_event_store` is
            // set. `Reset` is a "wipe all state" command and is rare in
            // production; persisting across it is a deliberate non-goal of
            // the FFI-path wiring.
            // V-82 — rebuild over the SAME FFI-shared active-account slot so
            // `NmpApp::active_account_handle()` keeps reading the slot the
            // rebuilt kernel writes (a bare `Kernel::new` would mint a fresh
            // slot and silently orphan the host's handle on every Reset).
            // Mirrors the routing-trace re-publish contract below: the shared
            // `Arc` outlives the discarded kernel.
            *ctx.kernel = Kernel::with_storage_path_and_account_slot(
                ctx.kernel.visible_limit(),
                None,
                Arc::clone(ctx.active_account_slot),
            );
            // V-82 — clear the shared active-account slot to match the fresh
            // kernel's empty `active_account` projection. The rebuilt kernel
            // only writes the slot on the next identity mutation (`set_accounts`),
            // so without this the slot would retain the pre-Reset pubkey and
            // `NmpApp::active_account_handle()` would report a stale account
            // while every other projection says "no account". Pre-V-82 the
            // host-observable post-Reset value was `None` (the discarded kernel
            // minted a fresh empty slot); clearing here preserves that. D6:
            // poisoned lock → silent no-op, matching the other slots' policy.
            if let Ok(mut guard) = ctx.active_account_slot.lock() {
                *guard = None;
            }
            if let Some(handle) = drops_handle {
                ctx.kernel.set_dispatch_drops_handle(handle);
            }
            if let Some(handle) = queue_depth_handle {
                ctx.kernel.set_queue_depth_handle(handle);
            }
            if let Some(handle) = event_observers_handle {
                ctx.kernel.set_event_observers_handle(handle);
            }
            if let Some(handle) = raw_event_observers_handle {
                ctx.kernel.set_raw_event_observers_handle(handle);
            }
            if let Some(handle) = snapshot_projection_handle {
                ctx.kernel.set_snapshot_projection_handle(handle);
            }
            if let Some(handle) = configured_relays_handle {
                ctx.kernel.set_app_relay_slot(handle);
            }
            // V-40 — re-bind the substrate `EventIngestDispatcher` slot
            // and the `DmInboxRelayLookup` handle on the rebuilt kernel.
            // The slots outlive the reset (shared `Arc`s with `NmpApp`);
            // re-binding ensures the rebuilt kernel sees the same per-NIP
            // parser registrations + DM-relay cache the registration path
            // mutated. Mirrors the initial bind in
            // `run_actor_with_observers`.
            ctx.kernel
                .set_ingest_dispatcher_slot(Arc::clone(ctx.ingest_dispatcher_slot));
            {
                let lookup = ctx
                    .dm_inbox_relays_slot
                    .lock()
                    .ok()
                    .map(|g| Arc::clone(&*g))
                    .unwrap_or_else(crate::substrate::empty_dm_inbox_relay_lookup);
                ctx.kernel.set_dm_inbox_relay_lookup(lookup);
            }
            {
                let lookup = ctx
                    .blocked_relays_slot
                    .lock()
                    .ok()
                    .map(|g| Arc::clone(&*g))
                    .unwrap_or_else(crate::substrate::empty_blocked_relay_lookup);
                ctx.kernel.set_blocked_relay_lookup(lookup);
            }
            {
                let kinds = ctx.bootstrap_self_kinds_slot.lock().ok().and_then(|g| {
                    g.as_ref()
                        .map(|v| v.iter().map(|n| *n as u32).collect::<Vec<u32>>())
                });
                ctx.kernel.set_bootstrap_self_kinds_override(kinds);
            }
            // D2 — re-install the coverage-gate hook on the rebuilt kernel.
            // The slot outlives the reset (shared `Arc` with `NmpApp`); reading
            // it here ensures the rebuilt lifecycle also enforces D2. Mirrors
            // the initial install in `run_actor_with_observers`.
            if let Some(hook) = ctx.coverage_hook_slot.lock().ok().and_then(|g| g.clone()) {
                ctx.kernel.lifecycle_mut().set_coverage_hook(hook);
            }
            if let Some(interceptor) = ctx
                .req_frame_interceptor_slot
                .lock()
                .ok()
                .and_then(|g| g.clone())
            {
                ctx.kernel
                    .lifecycle_mut()
                    .set_req_frame_interceptor(interceptor);
            }
            // V-51 phase 4 — re-publish the rebuilt kernel's routing-trace
            // projection clone into the shared slot. The previous projection
            // was attached to the now-discarded kernel; `Reset` is a "wipe
            // state" command and the reader contract is "the most recent
            // routing decisions of the live kernel".
            if let Ok(mut guard) = ctx.routing_trace_slot.lock() {
                *guard = Some(ctx.kernel.routing_trace());
            }
            // V-83 — re-publish the rebuilt kernel's `EventStore` handle clone.
            // `Reset` constructed a fresh kernel (and hence a fresh store) above;
            // without this the slot would retain a handle to the discarded
            // kernel's store and `NmpApp::event_by_id` would read stale (empty
            // post-wipe) data. Same publish-back-on-`Reset` contract as the
            // routing-trace projection above.
            if let Ok(mut guard) = ctx.event_store_slot.lock() {
                *guard = Some(ctx.kernel.event_store_handle());
            }
            // V-51 phase 5 — re-apply the per-app substrate-routing factory
            // against the rebuilt kernel. Same contract as the routing-trace
            // re-publish above: the previous router/cache pair was discarded
            // with the old kernel; the factory rebuilds against the fresh
            // projection so production composition survives a state wipe.
            if let Some(factory) = ctx
                .routing_substrate_slot
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(Arc::clone))
            {
                let observer: Arc<dyn crate::substrate::RoutingTraceObserver> =
                    ctx.kernel.routing_trace() as Arc<dyn crate::substrate::RoutingTraceObserver>;
                let (router, cache) = factory(observer);
                ctx.kernel.set_routing(router, cache);
            }
            // Spec §271 (2026-05-25) — re-apply the per-app
            // substrate-publish-resolver factory against the rebuilt kernel.
            // Same contract as the routing-substrate re-apply above: the
            // previous resolver was discarded with the old kernel; the
            // factory rebuilds against the fresh handles so production
            // composition survives a state wipe.
            if let Some(factory) = ctx
                .publish_resolver_slot
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(Arc::clone))
            {
                let resolver = factory(
                    ctx.kernel.event_store_handle(),
                    ctx.kernel.indexer_relays_handle(),
                    ctx.kernel.local_write_relays_handle(),
                    ctx.kernel.active_account_handle(),
                );
                ctx.kernel.set_publish_resolver(resolver);
            }
            // Re-register injected raw-event forwarding policies against the
            // rebuilt kernel. The prior observers captured handles from the
            // discarded kernel; re-running the factory preserves policy
            // registrations while keeping target selection out of core.
            crate::actor::raw_event_forwarder::register_raw_event_forward_policies(
                ctx.kernel,
                ctx.raw_event_observers_handle,
                ctx.pool,
                ctx.raw_event_forward_observer_ids,
                ctx.raw_event_forward_policy_slot,
            );
            *ctx.startup_sent = false;
            if *ctx.running {
                ctx.kernel.start();
                // D1 — first snapshot must reach the shell before any relay TCP
                // connection is dialed, so emit_now precedes spawn_missing_relays.
                emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
                spawn_missing_relays(
                    ctx.relay_controls,
                    ctx.slot_to_url,
                    ctx.pool,
                    ctx.kernel,
                    ctx.next_relay_generation,
                );
            }
            Some(Vec::new())
        }
        ActorCommand::PushInterest(interest) => {
            ctx.kernel.lifecycle_mut().registry_mut().push(interest);
            ctx.kernel.lifecycle_mut().enqueue_trigger(
                crate::subs::CompileTrigger::InvalidateCompile {
                    reason: crate::subs::InvalidateReason::External("push-interest".to_string()),
                },
            );
            Some(Vec::new())
        }
        ActorCommand::WithdrawInterest(id) => {
            ctx.kernel.lifecycle_mut().registry_mut().withdraw(&id);
            ctx.kernel.lifecycle_mut().enqueue_trigger(
                crate::subs::CompileTrigger::InvalidateCompile {
                    reason: crate::subs::InvalidateReason::External(
                        "withdraw-interest".to_string(),
                    ),
                },
            );
            Some(Vec::new())
        }
        ActorCommand::EnsureInterest { identity, interest } => {
            let newly_installed = ctx
                .kernel
                .lifecycle_mut()
                .registry_mut()
                .ensure_sub(identity, interest);
            if newly_installed {
                ctx.kernel.lifecycle_mut().enqueue_trigger(
                    crate::subs::CompileTrigger::InvalidateCompile {
                        reason: crate::subs::InvalidateReason::External(
                            "ensure-interest".to_string(),
                        ),
                    },
                );
            }
            Some(Vec::new())
        }
        ActorCommand::DropInterestOwner(identity) => {
            let removed = ctx
                .kernel
                .lifecycle_mut()
                .registry_mut()
                .drop_owner(&identity);
            if removed {
                ctx.kernel.lifecycle_mut().enqueue_trigger(
                    crate::subs::CompileTrigger::InvalidateCompile {
                        reason: crate::subs::InvalidateReason::External(
                            "drop-interest-owner".to_string(),
                        ),
                    },
                );
            }
            Some(Vec::new())
        }
        ActorCommand::OpenInterest {
            filter_json,
            consumer_id,
            scope,
        } => {
            // M2 (ADR-0042) — generic feed-subscription front door. Parse the
            // verbatim NIP-01 filter into an InterestShape, derive the
            // `(owner, key, scope)` identity from it, and run the same
            // ensure_sub + CompileTrigger body as the `EnsureInterest` arm.
            // D6: a malformed filter is a silent no-op (the FFI shim already
            // surfaced a toast before sending — see `nmp_app_open_interest`).
            if let Some((identity, interest)) =
                build_open_interest(&filter_json, &consumer_id, scope)
            {
                // ensure_sub + trigger-on-newly-installed live in the kernel
                // method so the open/close arms cannot drift on the invariant.
                let _ = ctx.kernel.open_interest_sub(identity, interest);
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::CloseInterest {
            filter_json,
            consumer_id,
            scope,
        } => {
            // M2 (ADR-0042) — detach one owner; drop the live sub on the last
            // leave. The `(owner, key, scope)` identity is reconstructed from
            // the SAME filter + consumer + scope the open used, so the
            // InterestShape hash lands on the same registry slot.
            if let Some((identity, _interest)) =
                build_open_interest(&filter_json, &consumer_id, scope)
            {
                let _ = ctx.kernel.close_interest_sub(&identity);
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::Barrier { ack } => {
            // V-105: test-support sync primitive. Sending `()` here proves
            // every command enqueued before this `Barrier` has been dispatched.
            // The `SyncSender::send` call is non-blocking (the bounded capacity
            // is 1); a broken receiver is a no-op (the test gave up already).
            let _ = ack.send(());
            Some(Vec::new())
        }
        ActorCommand::Shutdown => {
            close_relays(
                ctx.relay_controls,
                ctx.slot_to_url,
                ctx.pool,
                ctx.connected_relays,
                ctx.kernel,
            );
            ctx.connected_urls.clear();
            None
        }
        ActorCommand::Protocol(cmd) => {
            // Step 1.b — the open-seam dispatch arm. Debt C replaced the
            // prior 12-positional-closure bundle with typed capability
            // adapters (`KernelClock`/`LocalSignerAccess`/`DmInboxLookup`/
            // `ErrorSurface`/`ActionStageTracker`/`RecipientRelayLookup`).
            // Each adapter borrows a `RefCell`-wrapped reference to the
            // kernel or identity runtime; the kernel and identity types
            // stay crate-private (D0 — NIP crates name neither). Borrows
            // are released the moment `cmd.run` returns — the worker thread
            // the LNURL command spawns owns its own `Sender<ActorCommand>`
            // clone and never re-enters the context.
            //
            // V-38: the dispatch arm additionally attaches an `&mut Kernel`
            // and an outbound-frame sink so NIP-crate runtimes (today
            // `nmp-nip47`) can mutate the kernel synchronously and surface
            // relay frames the actor drains into `send_all_outbound`
            // without re-entering through the `send` channel.
            let tx = ctx.command_tx_self.clone();
            let send = move |c: crate::actor::ActorCommand| {
                // D6 — disconnected sender (post-Shutdown) is a benign
                // send-failure on the worker side; swallow as a no-op.
                let _ = tx.send(c);
            };
            // Snapshot the DM-inbox lookup Arc for the duration of this
            // dispatch arm. The `Arc<dyn DmInboxRelayLookup>` is the
            // production kind:10050 cache (`nmp_nip17::DmRelayCache`).
            let dm_lookup = ctx.kernel.dm_inbox_relays_arc();
            // The kernel + identity adapters share disjoint borrows of the
            // actor context via `RefCell`. `ProtocolCommand::run` is
            // single-threaded sync, so the inner `borrow`/`borrow_mut`
            // calls serialize naturally.
            //
            // V-38: the typed capability adapters borrow `ctx.kernel` via
            // `RefCell`; the V-38 `with_kernel` builder needs an exclusive
            // `&mut Kernel` borrow. The adapters and the direct kernel
            // borrow are mutually exclusive — the `with_kernel` borrow
            // begins after the adapters drop at end-of-block. We capture
            // the identity-runtime ref first (immutable) so it can outlive
            // the adapter scope; the kernel borrow is rebuilt in the
            // post-adapter block below.
            let identity_cell = std::cell::RefCell::new(&*ctx.identity);
            let kernel_cell = std::cell::RefCell::new(&mut *ctx.kernel);

            let clock = KernelClockAdapter {
                kernel: &kernel_cell,
            };
            let signers = LocalSignerAccessAdapter {
                identity: &identity_cell,
            };
            let errors = ErrorSurfaceAdapter {
                kernel: &kernel_cell,
            };
            let stages = ActionStageTrackerAdapter {
                kernel: &kernel_cell,
            };
            let recipients = RecipientRelayLookupAdapter {
                kernel: &kernel_cell,
            };

            // A second sender clone for the worker-thread surface. Cloning
            // a `mpsc::Sender` is cheap (atomic ref-count bump); the
            // dispatch arm always populates this slot in production.
            let worker_tx = ctx.command_tx_self.clone();
            let mut outbound: Vec<crate::relay::OutboundMessage> = Vec::new();
            let run_err = {
                let pctx = crate::substrate::ProtocolCommandContext::new(
                    crate::substrate::ProtocolCommandContextParts {
                        send: &send,
                        command_sender: worker_tx,
                        clock: &clock,
                        signers: &signers,
                        dms: &*dm_lookup,
                        errors: &errors,
                        stages: &stages,
                        recipients: &recipients,
                    },
                )
                .with_outbound(&mut outbound);
                // V-38: attach the kernel handle so wallet `ProtocolCommand`
                // bodies can drive kernel state (toast, persistent-sub
                // register, action-terminal record) directly. The kernel
                // borrow here is taken through the `RefCell` so the typed
                // adapters above continue to function during `cmd.run`.
                // Since `ProtocolCommand::run` is single-threaded sync, the
                // adapters' `RefCell` borrows and the `with_kernel` borrow
                // are sequenced naturally inside the command body.
                let mut kernel_ref = kernel_cell.borrow_mut();
                let mut pctx = pctx.with_kernel(&mut *kernel_ref);
                let res = cmd.run(&mut pctx);
                drop(pctx);
                drop(kernel_ref);
                res
            };
            if let Err(e) = run_err {
                tracing::warn!(error = %e, "ProtocolCommand returned error");
            }
            // Drop the adapter borrows before the emit so `emit_now` can
            // re-borrow `ctx.kernel` mutably. The `kernel_cell` /
            // `identity_cell` `RefCell` borrows are released when the
            // adapters drop at end-of-block — explicitly drop the
            // adapters here so the `emit_now` below sees a fully
            // released `ctx.kernel`. The `RefCell` owners themselves are
            // moved at function end (no explicit `drop` needed once the
            // adapters that borrowed them are dropped).
            drop(recipients);
            drop(stages);
            drop(errors);
            drop(signers);
            drop(clock);
            // V-41 + V-39+V-40 + V-38 — a `ProtocolCommand` body may have
            // mutated the kernel (the `Requested` stage write, a toast, a
            // recorded failure) or queued follow-up `ActorCommand`s
            // (`ShowToast` / `RecordActionFailure` / `PublishSignedEvent`).
            // Emit promptly so the next snapshot tick carries the visible
            // effect, mirroring the legacy `FetchLnurlInvoice` and
            // `SendGiftWrappedDm` arms' `emit_now` precedents.
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::IngestPreVerifiedEvents(events) => {
            // D4 (single writer per fact): actor thread is the sole mutator.
            // Routes each event through kernel.ingest_pre_verified_event under the
            // "diag-firehose-stress" sub-id.  Note: ingest_pre_verified_event does
            // NOT call should_store_event or ingest_timeline_event — it directly
            // calls store.insert + populates the read-cache (events HashMap + timeline).
            // sort_timeline() is deferred to after the loop to avoid O(n²·log n)
            // cost for large batches (e.g. S3: 100k events).
            for verified in events {
                ctx.kernel.ingest_pre_verified_event(
                    crate::relay::RelayRole::Content,
                    "diag-firehose-stress",
                    verified,
                );
            }
            // One sort after all events are ingested: O(n log n) not O(n²·log n).
            ctx.kernel.sort_timeline_deferred();
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
    }
}

/// Resolve a [`nmp_network::pool::RelayHandle`] back to the `(URL, role)`
/// pair the actor tracks in `relay_controls`. Returns `None` for a stale
/// handle — the slot may have been reopened (different generation) or the
/// caller may have already shut down the worker for this URL. Stale events
/// are dropped silently; the pool's translator already filters out events
/// whose slot generation no longer matches, so this is belt-and-braces.
fn resolve_handle<'a>(
    h: nmp_network::pool::RelayHandle,
    relay_controls: &'a HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &'a HashMap<u32, CanonicalRelayUrl>,
) -> Option<(&'a CanonicalRelayUrl, RelayRole)> {
    let url = slot_to_url.get(&h.slot())?;
    let control = relay_controls.get(url)?;
    if control.handle.generation() != h.generation() {
        return None;
    }
    Some((url, control.role))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_relay_event(
    event: PoolEvent,
    kernel: &mut Kernel,
    // V-38: substrate-generic interceptor slot — `nmp-nip47`'s wallet
    // runtime installs itself here to peek at kind:23195 NWC responses
    // before the kernel drops them as unknown kinds.
    relay_text_interceptor: &crate::substrate::RelayTextInterceptorSlot,
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    next_relay_generation: &mut u64,
    connected_relays: &mut HashSet<RelayRole>,
    connected_urls: &mut HashSet<CanonicalRelayUrl>,
    update_tx: &Sender<crate::update_envelope::UpdateFrameBytes>,
    last_emit: &mut Instant,
    startup_sent: &mut bool,
    running: bool,
) {
    match event {
        // ── Opened ───────────────────────────────────────────────────────
        // Pool→kernel handshake for "socket dial completed". Carries the
        // URL (the only `PoolEvent` variant that does) plus the handle's
        // generation — we look up the role from `relay_controls` keyed by
        // the canonical URL the pool reports (already canonical, since
        // `ensure_relay_worker` only ever hands canonical strings in).
        PoolEvent::Opened { h, url, .. } => {
            let canonical = CanonicalRelayUrl::parse_or_raw(&url);
            let Some(control) = relay_controls.get(&canonical) else {
                // No control row — stale event (worker spawned, then
                // RemoveRelay shut down the slot before `Opened` arrived).
                return;
            };
            if control.handle.generation() != h.generation() {
                return;
            }
            let role = control.role;
            connected_relays.insert(role);
            kernel.relay_connected_url(role, &url);
            // T116/G1 — reconnect-replay. The first `Opened` for a URL is
            // the initial dial; the startup path (`maybe_send_startup` /
            // `kernel.startup_requests()`) emits REQs there. Every
            // subsequent `Opened` after a `Failed`/`Closed` is a true
            // reconnect — the kernel's `wire_subs` for that URL were
            // evicted by `relay_closed` (T133), and the relay's
            // per-connection sub-id table is fresh, so we must re-emit
            // active sub-shapes. `kernel.replay_on_reconnect` consults
            // `SubscriptionLifecycle::handle_reconnect` (a pure read of
            // `current_plan`) and applies the T129 watermark per-shape so
            // `since` is bumped past already-stored events.
            //
            // D7 preserved: actor reports the OS-level transition; the
            // kernel decides what to replay and rewrites `since`.
            let is_reconnect = !connected_urls.insert(canonical.clone());
            if is_reconnect && running {
                let replay = kernel.replay_on_reconnect(role, &url);
                if !replay.is_empty() {
                    send_all_outbound(
                        relay_controls,
                        slot_to_url,
                        pool,
                        kernel,
                        next_relay_generation,
                        replay,
                    );
                }
            }
            if running {
                let publish_replay = kernel.mark_publish_relay_available(&url);
                if !publish_replay.is_empty() {
                    send_all_outbound(
                        relay_controls,
                        slot_to_url,
                        pool,
                        kernel,
                        next_relay_generation,
                        publish_replay,
                    );
                }
            }
            maybe_send_startup(
                running,
                startup_sent,
                connected_relays,
                relay_controls,
                slot_to_url,
                pool,
                kernel,
                next_relay_generation,
            );
            emit_now(kernel, running, update_tx, last_emit);
        }
        // ── Failed ───────────────────────────────────────────────────────
        // Pool→kernel "socket dial / mid-session failed". The pool decides
        // whether this is permanent (HTTP 401/403 → no reconnect) or
        // transient (transport reset → it will retry with backoff). The
        // kernel observable is the per-URL `retrying` mark either way; the
        // permanent-vs-transient distinction surfaces via the next
        // `Opened` (transient) or absence thereof (permanent).
        PoolEvent::Failed { h, error, .. } => {
            let Some((url, role)) = resolve_handle(h, relay_controls, slot_to_url) else {
                return;
            };
            let url = url.as_str().to_string();
            connected_relays.remove(&role);
            *startup_sent = false;
            // T105: scope the `retrying` mark to the specific socket that
            // failed — sibling sockets sharing this role lane are still live.
            kernel.relay_failed(role, &url, error.message);
            kernel.mark_publish_relay_unavailable(&url);
            emit_now(kernel, running, update_tx, last_emit);
        }
        // ── Closed ───────────────────────────────────────────────────────
        // Pool→kernel "socket torn down, no retry". Mirrors the legacy
        // `RelayEvent::Closed` arm one-to-one.
        PoolEvent::Closed { h, .. } => {
            let Some((url, role)) = resolve_handle(h, relay_controls, slot_to_url) else {
                return;
            };
            let url = url.as_str().to_string();
            connected_relays.remove(&role);
            *startup_sent = false;
            // T105: scope T133 wire-sub eviction to the closed socket's URL,
            // not the whole role lane (sibling sockets keep their subs).
            kernel.relay_closed(role, &url);
            kernel.mark_publish_relay_unavailable(&url);
            emit_now(kernel, running, update_tx, last_emit);
        }
        // ── Frame ────────────────────────────────────────────────────────
        // Pool→kernel inbound wire frame. The pool's translator already
        // converted `tungstenite::Message → RelayFrame` (and pre-classified
        // NIP-42 AUTH frames into `RelayFrame::Auth` in phase E); we
        // round-trip the `Auth` variant back to a `Text` frame so the
        // kernel's existing ingest path handles AUTH unchanged.
        PoolEvent::Frame { h, frame, .. } if running => {
            let Some((url, role)) = resolve_handle(h, relay_controls, slot_to_url) else {
                return;
            };
            let url_str = url.as_str().to_string();
            // V-38: peek at the text payload BEFORE kernel ingest so an
            // installed substrate-generic relay-text interceptor (today
            // `nmp-nip47`'s NWC runtime) can decode kind:23195 responses
            // the kernel itself drops as unknown kinds. The interceptor
            // filters by relay URL internally; uninteresting frames are a
            // single-lock no-op. D0: substrate-generic — no NIP-47 / NWC
            // nouns in nmp-core.
            let raw_text = match &frame {
                PoolFrame::Text(s) => Some(s.clone()),
                // Phase F: phase-E `RelayFrame::Auth` doesn't carry a
                // payload an interceptor would interpret; nothing to peek.
                _ => None,
            };
            let kernel_frame = pool_frame_to_relay_frame(frame);
            let mut outbound = kernel.handle_message(role, &url_str, kernel_frame);
            outbound.extend(kernel.pending_view_requests());
            // V-58: drain any backoff hints the kernel enqueued during
            // `handle_message` (e.g. from a rate-limited CLOSED) and forward
            // each one to the pool worker. The hint is URL-keyed; we look up
            // the handle via `relay_controls` the same way every other per-URL
            // dispatch does. Stale or missing handles are silently ignored.
            for (hint_url, hint) in kernel.take_backoff_hints() {
                let canonical = CanonicalRelayUrl::parse_or_raw(&hint_url);
                if let Some(control) = relay_controls.get(&canonical) {
                    let class = match hint {
                        BackoffHint::RateLimited => BackoffClass::RateLimited,
                    };
                    pool.set_backoff_hint(control.handle, class);
                }
            }
            if let Some(text) = raw_text {
                let interceptors = relay_text_interceptor
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                for interceptor in interceptors {
                    let extra = interceptor.on_relay_text(kernel, &url_str, &text);
                    outbound.extend(extra);
                }
            }
            send_all_outbound(
                relay_controls,
                slot_to_url,
                pool,
                kernel,
                next_relay_generation,
                outbound,
            );
        }
        PoolEvent::Frame { .. } => {}
        // ── Health ───────────────────────────────────────────────────────
        // Diagnostic snapshot; the kernel doesn't act on it (per-URL health
        // is M11). Reserved for future per-URL health-row writes.
        PoolEvent::Health { .. } => {}
    }
}

#[cfg(test)]
mod open_interest_tests {
    //! M2 (ADR-0042) — unit tests for `build_open_interest`, the shared body of
    //! the `OpenInterest` / `CloseInterest` dispatch arms: filter parse →
    //! `InterestShape` → `SubIdentity` + `Tailing` `LogicalInterest`, plus the
    //! end-to-end open → `ensure_sub` → `CompileTrigger` / close → `drop_owner`
    //! contract against a real `InterestRegistry`.
    use super::build_open_interest;
    use crate::planner::{InterestLifecycle, InterestScope};
    use crate::subs::InterestRegistry;

    #[test]
    fn parses_filter_into_tailing_interest_with_scope() {
        let (identity, interest) =
            build_open_interest(r#"{"kinds":[1,6],"authors":["aa"]}"#, "author-aa", 0)
                .expect("valid filter");

        // Lifecycle is always Tailing for a feed subscription.
        assert_eq!(interest.lifecycle, InterestLifecycle::Tailing);
        // scope 0 → ActiveAccount (re-route on switch) rides on the interest.
        assert_eq!(interest.scope, InterestScope::ActiveAccount);
        // Shape carried the app-supplied kinds verbatim (D0 — no substrate default).
        assert_eq!(interest.shape.kinds, [1u32, 6u32].into_iter().collect());
        assert_eq!(interest.shape.authors, ["aa".to_string()].into_iter().collect());
        // Owner is derived from the consumer_id.
        let _ = identity;
    }

    #[test]
    fn scope_one_maps_to_global() {
        let (_id, interest) =
            build_open_interest(r##"{"kinds":[1],"#t":["bitcoin"]}"##, "tag-bitcoin", 1).unwrap();
        assert_eq!(interest.scope, InterestScope::Global);
    }

    #[test]
    fn malformed_filter_is_none() {
        assert!(build_open_interest("not json", "c", 0).is_none());
        assert!(build_open_interest("[]", "c", 0).is_none());
    }

    #[test]
    fn same_filter_different_json_order_dedups_to_one_slot() {
        // The dedup contract: two opens of the same filter (different key/element
        // ordering) under the same consumer collapse to one registry slot.
        let mut reg = InterestRegistry::new();
        let (id_a, int_a) =
            build_open_interest(r#"{"kinds":[1,6],"authors":["aa","bb"]}"#, "c", 0).unwrap();
        let (id_b, int_b) =
            build_open_interest(r#"{"authors":["bb","aa"],"kinds":[6,1]}"#, "c", 0).unwrap();

        assert!(reg.ensure_sub(id_a, int_a), "first open installs");
        assert!(
            !reg.ensure_sub(id_b, int_b),
            "same filter+consumer is a no-op install (already present)"
        );
        assert_eq!(reg.len(), 1, "deduped to a single slot");
    }

    #[test]
    fn distinct_consumers_share_the_slot_and_last_close_drops_it() {
        // Two consumers, same filter → one live interest, refcount 2. The slot
        // survives the first close and is dropped on the last.
        let mut reg = InterestRegistry::new();
        let filter = r#"{"kinds":[1,6],"authors":["aa"]}"#;
        let (id1, int1) = build_open_interest(filter, "consumer-1", 0).unwrap();
        let (id2, int2) = build_open_interest(filter, "consumer-2", 0).unwrap();

        assert!(reg.ensure_sub(id1.clone(), int1), "consumer-1 installs");
        assert!(!reg.ensure_sub(id2.clone(), int2), "consumer-2 attaches");
        assert_eq!(reg.len(), 1);

        // First close: slot survives (consumer-2 still attached).
        let (close1, _) = build_open_interest(filter, "consumer-1", 0).unwrap();
        assert!(!reg.drop_owner(&close1), "slot survives first close");
        assert_eq!(reg.len(), 1);

        // Last close: slot dropped.
        let (close2, _) = build_open_interest(filter, "consumer-2", 0).unwrap();
        assert!(reg.drop_owner(&close2), "last close drops the slot");
        assert!(reg.is_empty());
    }

    #[test]
    fn active_account_and_global_scope_of_same_filter_are_distinct_slots() {
        // scope is folded into the SubKey so the same filter under ActiveAccount
        // (re-routes) and Global (account-agnostic) does not collide — they route
        // differently and must be distinct registry entries.
        let mut reg = InterestRegistry::new();
        let filter = r##"{"kinds":[1],"#t":["bitcoin"]}"##;
        let (id_active, int_active) = build_open_interest(filter, "c", 0).unwrap();
        let (id_global, int_global) = build_open_interest(filter, "c", 1).unwrap();

        assert!(reg.ensure_sub(id_active, int_active));
        assert!(
            reg.ensure_sub(id_global, int_global),
            "different scope → newly installed, not a dedup"
        );
        assert_eq!(reg.len(), 2);
    }

    // ── Kernel methods: parse → ensure_sub → CompileTrigger enqueued ──────────
    // These assert the load-bearing half the registry-only tests above cannot:
    // that a newly-installed interest enqueues a recompile trigger (no trigger →
    // the interest never compiles to a REQ on the wire) and that dedups / closes
    // gate the trigger correctly.

    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    #[test]
    fn open_interest_sub_installs_and_enqueues_trigger() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let before = kernel.lifecycle_mut().pending_trigger_count();

        let (identity, interest) =
            build_open_interest(r#"{"kinds":[1,6],"authors":["aa"]}"#, "author-aa", 0).unwrap();
        let newly_installed = kernel.open_interest_sub(identity, interest);

        assert!(newly_installed, "first open installs the slot");
        assert_eq!(
            kernel.lifecycle_mut().pending_trigger_count(),
            before + 1,
            "a newly-installed interest enqueues exactly one recompile trigger"
        );
    }

    #[test]
    fn open_interest_sub_dedup_does_not_re_enqueue() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let filter = r#"{"kinds":[1,6],"authors":["aa"]}"#;

        let (id1, int1) = build_open_interest(filter, "consumer-1", 0).unwrap();
        assert!(kernel.open_interest_sub(id1, int1));
        let after_first = kernel.lifecycle_mut().pending_trigger_count();

        // Second owner on the SAME (scope,key) slot: attaches but does NOT
        // re-install, so no second trigger (idempotent — would otherwise churn
        // the compiler on every re-mount).
        let (id2, int2) = build_open_interest(filter, "consumer-2", 0).unwrap();
        assert!(!kernel.open_interest_sub(id2, int2), "second owner attaches");
        assert_eq!(
            kernel.lifecycle_mut().pending_trigger_count(),
            after_first,
            "attaching a second owner must not re-enqueue a trigger"
        );
    }

    #[test]
    fn close_interest_sub_enqueues_trigger_only_on_last_owner() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let filter = r#"{"kinds":[1,6],"authors":["aa"]}"#;

        let (id1, int1) = build_open_interest(filter, "consumer-1", 0).unwrap();
        let (id2, int2) = build_open_interest(filter, "consumer-2", 0).unwrap();
        kernel.open_interest_sub(id1, int1);
        kernel.open_interest_sub(id2, int2);
        let after_opens = kernel.lifecycle_mut().pending_trigger_count();

        // First close: slot survives (consumer-2 still attached) → no trigger.
        let (close1, _) = build_open_interest(filter, "consumer-1", 0).unwrap();
        assert!(!kernel.close_interest_sub(&close1), "slot survives");
        assert_eq!(
            kernel.lifecycle_mut().pending_trigger_count(),
            after_opens,
            "a non-final close does not enqueue a trigger"
        );

        // Last close: slot dropped → exactly one trigger.
        let (close2, _) = build_open_interest(filter, "consumer-2", 0).unwrap();
        assert!(kernel.close_interest_sub(&close2), "last close drops slot");
        assert_eq!(
            kernel.lifecycle_mut().pending_trigger_count(),
            after_opens + 1,
            "the final close enqueues exactly one recompile trigger"
        );
    }
}

#[cfg(test)]
mod nip65_auto_publish_tests {
    //! End-to-end tests for the NIP-65 auto-publish piggyback on
    //! `AddRelay` / `RemoveRelay`.
    //!
    //! Builder unit tests live next to the builder
    //! (`actor::commands::relays::tests`). These tests pin the wiring —
    //! that the dispatch arms actually invoke the builder, gate on the
    //! active signer, skip no-op edits, and route through
    //! `publish_unsigned_event` (i.e. the kind:10002 frame lands in the
    //! outbound `EVENT` stream the same way every other publish does).
    //!
    //! Closing the gap the PR title makes load-bearing: without these
    //! tests, a future refactor that drops the `maybe_publish_relay_list_after_edit`
    //! call would pass every other unit test silently.
    //!
    //! These tests use a known dev nsec — never wired to any real
    //! relay — to drive `IdentityRuntime` so `active_pubkey()` is `Some`.
    use super::*;
    use crate::actor::commands::{
        add_relay, add_signer, new_bunker_handshake_slot, remove_relay, IdentityRuntime,
    };
    use crate::actor::SignerSource;
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    /// Throwaway nsec — generated for tests only, never on the network.
    /// Same dev key the conformance harness round-trip tests
    /// (`tests/nip_tag_conformance.rs`) and the remote-signer tests
    /// (`actor/commands/remote_signer_tests.rs`) use. Reusing it here
    /// keeps the test fixture surface small.
    const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

    fn fresh_kernel() -> Kernel {
        Kernel::new(DEFAULT_VISIBLE_LIMIT)
    }

    fn fresh_identity() -> IdentityRuntime {
        use crate::actor::new_bunker_connection_state_slot;
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            new_bunker_connection_state_slot(),
        )
    }

    fn signed_in_identity(kernel: &mut Kernel) -> IdentityRuntime {
        let mut identity = fresh_identity();
        add_signer(
            &mut identity,
            kernel,
            SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
            true,
            false,
        );
        assert!(
            identity.active_pubkey().is_some(),
            "add_signer(LocalNsec, make_active) must produce an active account",
        );
        identity
    }

    /// Helper: count `["EVENT", { "kind": 10002, ... }]` frames in an
    /// outbound batch. Mirrors the conformance harness shape check —
    /// outbound text is a raw wire frame, so we string-search for the
    /// outer `["EVENT"` and a kind:10002 marker.
    fn count_kind_10002_frames(outbound: &[crate::relay::OutboundMessage]) -> usize {
        outbound
            .iter()
            .filter(|m| m.text.starts_with("[\"EVENT\""))
            .filter(|m| {
                // The wire shape is `["EVENT", {"kind":10002,...}]` (no
                // SUBSCRIPTION-ID prefix variant — kind:10002 routes
                // through the Auto outbox, not a REQ).
                let parsed: serde_json::Value = match serde_json::from_str(&m.text) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                parsed
                    .as_array()
                    .and_then(|arr| arr.get(1))
                    .and_then(|ev| ev.get("kind"))
                    .and_then(serde_json::Value::as_u64)
                    == Some(10002)
            })
            .count()
    }

    #[test]
    fn add_relay_with_active_signer_publishes_kind_10002() {
        // Headline assertion the PR title makes: a real AddRelay edit by a
        // signed-in user produces a kind:10002 frame.
        let mut kernel = fresh_kernel();
        let mut identity = signed_in_identity(&mut kernel);
        let mut pending = Vec::new();

        // Capture the projection BEFORE the mutation, as the dispatch arm
        // does, then mutate and call the helper directly.
        let before = kernel.configured_relays_snapshot().to_vec();
        let added = add_relay(&mut kernel, "wss://relay.example", "both");
        assert!(added.is_some(), "add_relay must accept a valid wss:// URL");

        let outbound =
            maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
        assert!(
            count_kind_10002_frames(&outbound) >= 1,
            "AddRelay with an active signer must re-publish kind:10002. \
             Outbound frames were: {:?}",
            outbound.iter().map(|m| &m.text).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn add_relay_without_active_signer_does_not_publish() {
        // Guard 1: a relay edit while signed out must NOT try to publish
        // (and must NOT set the no-account error toast).
        let mut kernel = fresh_kernel();
        let mut identity = fresh_identity();
        let mut pending = Vec::new();

        let before = kernel.configured_relays_snapshot().to_vec();
        add_relay(&mut kernel, "wss://relay.example", "both");

        let outbound =
            maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
        assert_eq!(
            count_kind_10002_frames(&outbound),
            0,
            "without an active signer, no kind:10002 must be published",
        );
        assert!(
            kernel.last_error_toast_snapshot().is_none(),
            "signed-out relay edits MUST NOT poison the toast slot \
             (toast_no_account would be wrong observable here)",
        );
    }

    #[test]
    fn add_relay_no_op_does_not_republish() {
        // Guard 2: re-adding the same URL with the same role is a no-op on
        // the projection. The dispatch arm MUST skip the re-publish in
        // that case — otherwise every duplicate FFI call burns a relay
        // write and bumps the kind:10002 timestamp for nothing.
        let mut kernel = fresh_kernel();
        let mut identity = signed_in_identity(&mut kernel);
        let mut pending = Vec::new();

        // First add — projection changes; this would publish.
        add_relay(&mut kernel, "wss://relay.example", "both");

        // Second add — identical role, no projection change.
        let before = kernel.configured_relays_snapshot().to_vec();
        add_relay(&mut kernel, "wss://relay.example", "both");

        let outbound =
            maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
        assert_eq!(
            count_kind_10002_frames(&outbound),
            0,
            "re-adding the same URL+role MUST NOT re-publish kind:10002 \
             (projection unchanged → no semantic change)",
        );
    }

    #[test]
    fn remove_relay_nonexistent_does_not_republish() {
        // Guard 2 (mirror): removing a URL that was never present is a
        // no-op on the projection. The dispatch arm MUST skip the
        // re-publish.
        let mut kernel = fresh_kernel();
        let mut identity = signed_in_identity(&mut kernel);
        let mut pending = Vec::new();

        // Seed one row so the projection is non-empty (otherwise guard 3
        // would also trip and we couldn't distinguish guard-2 from guard-3).
        add_relay(&mut kernel, "wss://relay.example", "both");

        let before = kernel.configured_relays_snapshot().to_vec();
        remove_relay(&mut kernel, "wss://other.example");

        let outbound =
            maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
        assert_eq!(
            count_kind_10002_frames(&outbound),
            0,
            "removing an absent URL MUST NOT re-publish kind:10002",
        );
    }

    #[test]
    fn remove_relay_existing_does_republish() {
        // Symmetric to `add_relay_with_active_signer_publishes_kind_10002`:
        // a real removal that mutates the projection must produce a
        // kind:10002 reflecting the new (smaller) set. This is the half
        // the PR is named for — clients reading the relay graph see the
        // removed relay leave the user's outbox without needing a manual
        // dispatch.
        let mut kernel = fresh_kernel();
        let mut identity = signed_in_identity(&mut kernel);
        let mut pending = Vec::new();

        // Seed two rows so the post-removal projection still has at least
        // one NIP-65-eligible row — otherwise guard 3 (don't publish
        // empty kind:10002) would correctly skip the publish.
        add_relay(&mut kernel, "wss://keep.example", "both");
        add_relay(&mut kernel, "wss://drop.example", "both");

        let before = kernel.configured_relays_snapshot().to_vec();
        remove_relay(&mut kernel, "wss://drop.example");

        let outbound =
            maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
        assert!(
            count_kind_10002_frames(&outbound) >= 1,
            "removing an existing URL must re-publish kind:10002 with \
             the remaining set. Outbound frames were: {:?}",
            outbound.iter().map(|m| &m.text).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn empty_projection_after_remove_does_not_republish() {
        // Guard 3: removing the user's last NIP-65-eligible row leaves
        // the projection empty. We must NOT publish an empty kind:10002
        // because `ingest_relay_list` treats that as "clear my NIP-65
        // metadata" (destructive — see kernel/ingest/relay_list.rs:31).
        // The user explicitly removing a relay is NOT the same intent as
        // "wipe my NIP-65 outbox"; that needs its own explicit verb.
        let mut kernel = fresh_kernel();
        let mut identity = signed_in_identity(&mut kernel);
        let mut pending = Vec::new();

        add_relay(&mut kernel, "wss://only.example", "both");

        let before = kernel.configured_relays_snapshot().to_vec();
        remove_relay(&mut kernel, "wss://only.example");
        assert!(
            kernel.configured_relays_snapshot().is_empty(),
            "test precondition: projection must be empty after removing the only row"
        );

        let outbound =
            maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
        assert_eq!(
            count_kind_10002_frames(&outbound),
            0,
            "removing the user's last NIP-65-eligible row MUST NOT \
             publish an empty kind:10002 (that would clear the \
             author_relay_lists cache on ingest — destructive)",
        );
    }
}

#[cfg(test)]
mod sign_return_tests {
    //! D13 sign-and-return — unit tests for the two pure helpers the
    //! `SignEventForReturn` dispatch arm relies on: `build_unsigned_for_return`
    //! (host draft → `UnsignedEvent`, filling pubkey + clock-stamped
    //! `created_at`) and `signed_event_to_json` (kernel `SignedEvent` → the flat
    //! NIP-01 event JSON the host base64-encodes for a Blossom auth header).
    use super::{build_unsigned_for_return, signed_event_to_json};
    use crate::substrate::{SignedEvent, UnsignedEvent};

    #[test]
    fn build_unsigned_fills_pubkey_and_restamps_created_at() {
        let draft = r#"{"kind":24242,"content":"Upload image","tags":[["t","upload"],["x","deadbeef"]],"created_at":111}"#;
        let unsigned = build_unsigned_for_return(draft, "signerpub", 999).expect("valid draft");
        // pubkey comes from the resolved signer, not the draft (the draft has none).
        assert_eq!(unsigned.pubkey, "signerpub");
        // created_at is re-stamped from the kernel clock (D7), ignoring the draft's 111.
        assert_eq!(unsigned.created_at, 999);
        assert_eq!(unsigned.kind, 24242);
        assert_eq!(unsigned.content, "Upload image");
        assert_eq!(
            unsigned.tags,
            vec![
                vec!["t".to_string(), "upload".to_string()],
                vec!["x".to_string(), "deadbeef".to_string()],
            ]
        );
    }

    #[test]
    fn build_unsigned_defaults_tags_to_empty_when_absent() {
        let unsigned =
            build_unsigned_for_return(r#"{"kind":1,"content":"hi"}"#, "pk", 5).expect("valid");
        assert!(unsigned.tags.is_empty(), "absent tags default to empty");
    }

    #[test]
    fn build_unsigned_rejects_missing_kind() {
        let err = build_unsigned_for_return(r#"{"content":"x"}"#, "pk", 0)
            .expect_err("missing kind is rejected");
        assert!(err.contains("kind"), "error names the missing field: {err}");
    }

    #[test]
    fn build_unsigned_rejects_missing_content() {
        let err = build_unsigned_for_return(r#"{"kind":1}"#, "pk", 0)
            .expect_err("missing content is rejected");
        assert!(err.contains("content"), "error names the missing field: {err}");
    }

    #[test]
    fn build_unsigned_rejects_malformed_json() {
        assert!(
            build_unsigned_for_return("not json", "pk", 0).is_err(),
            "malformed JSON is rejected (surfaced as an Err verdict, never a panic)"
        );
    }

    #[test]
    fn signed_event_to_json_produces_flat_nip01_shape() {
        let signed = SignedEvent {
            id: "aa".repeat(32),
            sig: "bb".repeat(64),
            unsigned: UnsignedEvent {
                pubkey: "cc".repeat(32),
                kind: 24242,
                tags: vec![vec!["t".to_string(), "upload".to_string()]],
                content: "Upload image".to_string(),
                created_at: 1234,
            },
        };
        let json: serde_json::Value =
            serde_json::from_str(&signed_event_to_json(&signed)).expect("valid JSON");
        // Flat NIP-01 shape — NOT nested under `unsigned` (the kernel serde shape).
        assert_eq!(json.get("id").and_then(|v| v.as_str()), Some(signed.id.as_str()));
        assert_eq!(
            json.get("pubkey").and_then(|v| v.as_str()),
            Some(signed.unsigned.pubkey.as_str())
        );
        assert_eq!(json.get("kind").and_then(serde_json::Value::as_u64), Some(24242));
        assert_eq!(json.get("created_at").and_then(serde_json::Value::as_u64), Some(1234));
        assert_eq!(json.get("sig").and_then(|v| v.as_str()), Some(signed.sig.as_str()));
        assert_eq!(
            json.get("content").and_then(|v| v.as_str()),
            Some("Upload image")
        );
        assert!(
            json.get("unsigned").is_none(),
            "the wire shape is flat — no `unsigned` nesting"
        );
    }
}
