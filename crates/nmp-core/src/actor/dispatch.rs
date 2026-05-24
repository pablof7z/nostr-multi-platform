//! Command + relay-event dispatch reducers.
//!
//! Split out of `mod.rs` to keep both files under the 300-LOC soft cap.
//! `dispatch_command` resolves an [`ActorCommand`] into outbound relay
//! messages (or `None` for shutdown); `handle_relay_event` folds a
//! [`RelayEvent`] into the kernel + connection bookkeeping. No behavior
//! change — pure move of the two reducers off the actor loop.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zeroize::Zeroizing;

use crate::ffi::{MlsLocalNsecSlot, Nip17LocalKeysSlot};
use crate::kernel::Kernel;
use crate::substrate::HostOpHandlerSlot;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole};
use nmp_network::relay_worker::RelayEvent;

use crate::kernel::RelayFrame;
use tungstenite::Message;

/// Convert a native [`tungstenite::Message`] into the wire-transport-agnostic
/// [`RelayFrame`] consumed by `Kernel::handle_message`.
///
/// Step 8 phase A: this conversion bridges `nmp-network`'s wire-message type
/// to the kernel's frame enum. It lives in `nmp-core` (not `nmp-network`)
/// because the `RelayFrame` enum is owned by the kernel layer — the
/// transport crate must not name kernel value types (D0 / step-8 boundary).
///
/// V-01 Phase 1c original placement: the kernel no longer names `tungstenite`,
/// so the conversion happens at the actor seam (the only native-feature
/// site that owns both vocabularies).
/// `Message::Frame` (raw-frame) maps to [`RelayFrame::Binary`] — the kernel's
/// only observable for non-text payloads is the bytes counter.
fn tungstenite_message_to_relay_frame(message: Message) -> RelayFrame {
    match message {
        Message::Text(text) => RelayFrame::Text(text),
        Message::Binary(bytes) => RelayFrame::Binary(bytes),
        Message::Ping(_) => RelayFrame::Ping,
        Message::Pong(_) => RelayFrame::Pong,
        Message::Close(frame) => RelayFrame::Close(frame.map(|f| f.reason.to_string())),
        Message::Frame(_) => RelayFrame::Binary(Vec::new()),
    }
}
use crate::subs::PlanCoverageHook;

use super::commands::{self, IdentityRuntime, LifecycleObserverSlot};
// D0: NIP-47 NWC is an app noun — `WalletRuntime` only exists with `wallet`.
#[cfg(feature = "wallet")]
use super::commands::WalletRuntime;
use crate::kernel_action::dispatch_kernel_action;
use super::pending_sign::PendingSign;
use super::relay_mgmt::{
    close_relays, ensure_relay_worker, maybe_send_startup, send_all_outbound,
    shutdown_relay_worker, spawn_missing_relays,
};
use super::session_persistence;
use super::tick::{emit_now, maybe_emit_after_dispatch};
use super::{ActorCommand, RelayControl};
use crate::capability_socket::CapabilityCallbackSlot;

/// Sync every host-readable local-key mirror to the current active account.
///
/// Two parallel slots track the active account's secret material on every
/// identity mutation:
///
/// * `mls_local_nsec` — bech32 `nsec1…` wrapped in [`Zeroizing`] so the
///   previous string is wiped from the heap on overwrite.
/// * `nip17_local_keys` — the parsed `nostr::Keys` for NIP-17 DM gift-wrap
///   decryption. `Keys` zeroizes its own secret on drop, so no extra wrapper
///   is needed.
///
/// Both derive from `identity.active_keys()`, so they always change together.
/// Each slot is locked, written, and dropped sequentially — there is no
/// cross-slot atomicity contract (a host that races a snapshot read against
/// an identity switch may briefly observe one slot updated and the other not;
/// the next snapshot tick reconciles).
///
/// Called synchronously BEFORE `maybe_emit_after_dispatch` (and before
/// `emit_now` on the `Start` arm) so the slots are visible to host callbacks
/// before any snapshot fires.
fn update_local_key_slots(
    identity: &IdentityRuntime,
    nsec_slot: &MlsLocalNsecSlot,
    nip17_keys_slot: &Nip17LocalKeysSlot,
) {
    if let Ok(mut guard) = nsec_slot.lock() {
        *guard = identity.active_nsec_bech32().map(Zeroizing::new);
    }
    if let Ok(mut guard) = nip17_keys_slot.lock() {
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
/// the local `RelayEditRow` projection and dialed / dropped sockets, but
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
///    kind:10002 with `r` tags. `build_relay_list_event_from_edit_rows`
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
    projection_before: &[crate::kernel::RelayEditRow],
    pending_signs: &mut Vec<super::pending_sign::PendingSign>,
) -> Vec<OutboundMessage> {
    // Guard 1: must have an active signer.
    if identity.active_pubkey().is_none() {
        return Vec::new();
    }
    // Guard 2: skip on no-op projection change.
    let projection_after = kernel.relay_edit_rows_snapshot();
    if projection_after == projection_before {
        return Vec::new();
    }
    // Guard 3: skip when the projection has no NIP-65 expression.
    let Some(unsigned) = commands::build_relay_list_event_from_edit_rows(projection_after) else {
        return Vec::new();
    };
    commands::publish_unsigned_event(identity, kernel, unsigned, None, pending_signs)
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
    // D0: NIP-47 NWC is an app noun — only present with the `wallet` feature.
    #[cfg(feature = "wallet")]
    pub(super) wallet: &'a mut WalletRuntime,
    pub(super) relay_controls: &'a mut HashMap<CanonicalRelayUrl, RelayControl>,
    pub(super) relay_tx: &'a Sender<RelayEvent>,
    pub(super) connected_relays: &'a mut HashSet<RelayRole>,
    pub(super) connected_urls: &'a mut HashSet<CanonicalRelayUrl>,
    pub(super) update_tx: &'a Sender<String>,
    pub(super) last_emit: &'a mut Instant,
    pub(super) next_relay_generation: &'a mut u64,
    pub(super) running: &'a mut bool,
    pub(super) emit_hz: &'a mut u32,
    pub(super) startup_sent: &'a mut bool,
    /// Derived per-call value (`all_relays_connected(...)`), not a borrow.
    pub(super) relays_ready: bool,
    pub(super) lifecycle_observer: &'a LifecycleObserverSlot,
    pub(super) mls_local_nsec: &'a MlsLocalNsecSlot,
    /// NIP-17 DM-inbox decryption key seam — the active account's local
    /// `nostr::Keys`. Updated alongside `mls_local_nsec` at every identity
    /// mutation. See [`update_local_key_slots`].
    pub(super) nip17_local_keys: &'a Nip17LocalKeysSlot,
    pub(super) capability_callback: &'a CapabilityCallbackSlot,
    pub(super) pending_signs: &'a mut Vec<PendingSign>,
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
    /// D2 — coverage-gate hook slot. Read by the `Reset` arm to re-install
    /// the hook on the rebuilt kernel (mirrors initial install in
    /// `run_actor_with_observers`).
    pub(super) coverage_hook_slot: &'a Arc<Mutex<Option<PlanCoverageHook>>>,
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
    pub(super) dm_inbox_relays_slot:
        &'a Arc<Mutex<Arc<dyn crate::substrate::DmInboxRelayLookup>>>,
}

pub(super) fn dispatch_command(
    command: ActorCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match command {
        ActorCommand::Start {
            visible_limit,
            emit_hz: hz,
        } => {
            *ctx.running = true;
            *ctx.emit_hz = hz;
            *ctx.startup_sent = false;
            ctx.kernel.set_visible_limit(visible_limit);
            commands::ensure_default_onboarding_relays(ctx.kernel);
            ctx.kernel.start();
            let mut outbound = session_persistence::restore_active_session(
                ctx.identity,
                ctx.kernel,
                ctx.capability_callback,
                ctx.relays_ready,
            );
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.nip17_local_keys);
            spawn_missing_relays(
                ctx.relay_controls,
                ctx.relay_tx,
                ctx.kernel,
                ctx.next_relay_generation,
            );
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
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
        ActorCommand::OpenAuthor { pubkey } => {
            let outbound = ctx.kernel.open_author(pubkey, ctx.relays_ready);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::OpenThread { event_id } => {
            let outbound = ctx.kernel.open_thread(event_id, ctx.relays_ready);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::OpenFirehoseTag { tag } => {
            let outbound = ctx.kernel.open_firehose_tag(tag, ctx.relays_ready);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::ClaimProfile {
            pubkey,
            consumer_id,
        } => {
            let outbound = ctx
                .kernel
                .claim_profile(pubkey, consumer_id, ctx.relays_ready);
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
        ActorCommand::SignInNsec { secret } => {
            // `secret` is `Zeroizing<String>`; pass the borrowed `&str` and let
            // the wrapper wipe the plaintext when it drops at end of scope.
            let outbound =
                commands::sign_in_nsec(ctx.identity, ctx.kernel, secret.as_str(), ctx.relays_ready);
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.nip17_local_keys);
            session_persistence::persist_current_active_session(
                ctx.identity,
                ctx.capability_callback,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::SignInBunker { uri } => {
            commands::sign_in_bunker(ctx.identity, ctx.kernel, &uri);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::CreateAccount {
            profile,
            relays,
            mls,
        } => {
            let outbound = commands::create_account(
                ctx.identity,
                ctx.kernel,
                ctx.relays_ready,
                &profile,
                &relays,
                mls,
            );
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.nip17_local_keys);
            session_persistence::persist_current_active_session(
                ctx.identity,
                ctx.capability_callback,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::SwitchActive { identity_id } => {
            let outbound =
                commands::switch_active(ctx.identity, ctx.kernel, &identity_id, ctx.relays_ready);
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.nip17_local_keys);
            session_persistence::persist_current_active_session(
                ctx.identity,
                ctx.capability_callback,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::RemoveAccount { identity_id } => {
            let outbound = commands::remove_account(ctx.identity, ctx.kernel, &identity_id);
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.nip17_local_keys);
            session_persistence::forget_account(&identity_id, ctx.capability_callback);
            session_persistence::persist_current_active_session(
                ctx.identity,
                ctx.capability_callback,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::AddRemoteSigner { handle } => {
            let remote_identity_id = handle.pubkey_hex();
            let remote_payload_json = handle.persistence_payload_json();
            let outbound =
                commands::add_remote_signer(ctx.identity, ctx.kernel, handle, ctx.relays_ready);
            if let Some(payload_json) = remote_payload_json {
                session_persistence::persist_remote_signer_payload(
                    &remote_identity_id,
                    &payload_json,
                    ctx.capability_callback,
                );
            }
            update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.nip17_local_keys);
            session_persistence::persist_current_active_session(
                ctx.identity,
                ctx.capability_callback,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::BunkerHandshakeProgress { stage, message } => {
            commands::bunker_handshake_progress(ctx.identity, ctx.kernel, stage, message);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::PublishNote {
            content,
            reply_to_id,
            target,
            correlation_id,
        } => {
            // Record Requested at dequeue time. Downstream arms record
            // Publishing (engine accept) and Accepted/Failed (terminal).
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::publish_note(
                ctx.identity,
                ctx.kernel,
                &content,
                reply_to_id.as_deref(),
                target,
                correlation_id,
                ctx.pending_signs,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishRawEvent {
            kind,
            tags,
            content,
            target,
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
            // same identity-kind support as `PublishNote`/`PublishProfile`.
            let outbound = match target {
                crate::publish::PublishTarget::Auto => commands::publish_unsigned_event(
                    ctx.identity,
                    ctx.kernel,
                    unsigned,
                    correlation_id,
                    ctx.pending_signs,
                ),
                crate::publish::PublishTarget::Explicit { relays } => {
                    commands::publish_unsigned_event_to_relays(
                        ctx.identity,
                        ctx.kernel,
                        unsigned,
                        relays,
                        correlation_id,
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
                ctx.pending_signs,
            );
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishUnsignedEventToRelays {
            mut event,
            relays,
            correlation_id,
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
            // the NIP-65 read/write distinction lives in RelayEditRow, not in
            // the transport pool key (T105). ensure_relay_worker is idempotent —
            // a role-edit for an already-connected URL is a harmless no-op.
            //
            // T-nip65-auto-publish: snapshot the projection BEFORE the mutation
            // so we can compare-and-skip the re-publish when the call was a
            // pure no-op (re-adding the same URL with the same role). Without
            // this every harmless re-add re-published kind:10002 and burned a
            // relay write.
            let projection_before = ctx.kernel.relay_edit_rows_snapshot().to_vec();
            let mut outbound = Vec::new();
            if let Some(canonical_url) = commands::add_relay(ctx.kernel, &url, &role) {
                ensure_relay_worker(
                    ctx.relay_controls,
                    ctx.relay_tx,
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
            // and RelayEditRow.url always agree regardless of how the FFI caller
            // spelled the URL. Shutdown the worker first so the socket is closed
            // before the projection row is removed. Idempotent: if no worker exists
            // for the URL, shutdown_relay_worker returns false and the projection
            // mutation still proceeds normally (D6: no silent drops).
            //
            // T-nip65-auto-publish: same compare-and-skip as `AddRelay` above.
            // Removing a URL that was never present is a no-op and must NOT
            // re-publish kind:10002.
            let projection_before = ctx.kernel.relay_edit_rows_snapshot().to_vec();
            shutdown_relay_worker(ctx.relay_controls, &url);
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
        ActorCommand::OpenTimeline => {
            let outbound = commands::open_timeline(ctx.identity, ctx.kernel, ctx.relays_ready);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        #[cfg(feature = "wallet")]
        ActorCommand::WalletConnect { uri } => {
            let outbound = commands::wallet_connect(ctx.wallet, ctx.kernel, &uri);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        #[cfg(feature = "wallet")]
        ActorCommand::WalletDisconnect => {
            let outbound = commands::wallet_disconnect(ctx.wallet, ctx.kernel);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        #[cfg(feature = "wallet")]
        ActorCommand::WalletPayInvoice {
            bolt11,
            amount_msats,
            correlation_id,
        } => {
            // Terminal Accepted/Failed arrives later via handle_nwc_text
            // when the wallet's kind:23195 response settles.
            if let Some(ref cid) = correlation_id {
                ctx.kernel.record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            }
            let outbound = commands::wallet_pay_invoice(
                ctx.wallet,
                ctx.kernel,
                &bolt11,
                amount_msats,
                correlation_id,
            );
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        // V-41 — the legacy `FetchLnurlInvoice` arm is deleted. The LNURL
        // fetcher now lives in `nmp_nip57::lnurl::FetchLnurlInvoiceCommand`
        // and dispatches through `ActorCommand::Protocol` (below). The
        // pre-existing `Requested` stage recording (gated on
        // `correlation_id`) and the post-dispatch `emit_now` both move
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
        ActorCommand::RecordActionSuccess { correlation_id } => {
            // Symmetric counterpart to RecordActionFailure: off-thread workers
            // (e.g. the LNURL-pay HTTP worker) fan success back through the
            // actor channel. Writes `Accepted` to `action_stages` and a
            // terminal verdict to `action_results`.
            ctx.kernel.record_action_success(correlation_id);
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
                ctx.kernel.record_action_success(correlation_id);
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
        ActorCommand::Stop => {
            *ctx.running = false;
            *ctx.startup_sent = false;
            close_relays(ctx.relay_controls, ctx.connected_relays, ctx.kernel);
            // T116/G1 — clear reconnect-replay discriminator so a subsequent
            // Start replays cleanly (every URL appears as a first-connect).
            ctx.connected_urls.clear();
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::Reset => {
            close_relays(ctx.relay_controls, ctx.connected_relays, ctx.kernel);
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
            let relay_edit_rows_handle = ctx.kernel.take_relay_edit_rows_handle_for_reset();
            // NOTE: the FFI-supplied LMDB `storage_path` (from
            // `nmp_app_set_storage_path`) is NOT re-threaded here — `Reset`
            // rebuilds the kernel with the in-memory store unless the
            // `NMP_LMDB_PATH` env-var fallback in `build_event_store` is
            // set. `Reset` is a "wipe all state" command and is rare in
            // production; persisting across it is a deliberate non-goal of
            // the FFI-path wiring.
            *ctx.kernel = Kernel::new(ctx.kernel.visible_limit());
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
            if let Some(handle) = relay_edit_rows_handle {
                ctx.kernel.set_relay_edit_rows_handle(handle);
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
            // D2 — re-install the coverage-gate hook on the rebuilt kernel.
            // The slot outlives the reset (shared `Arc` with `NmpApp`); reading
            // it here ensures the rebuilt lifecycle also enforces D2. Mirrors
            // the initial install in `run_actor_with_observers`.
            if let Some(hook) = ctx
                .coverage_hook_slot
                .lock()
                .ok()
                .and_then(|g| g.clone())
            {
                ctx.kernel.lifecycle_mut().set_coverage_hook(hook);
            }
            *ctx.startup_sent = false;
            if *ctx.running {
                ctx.kernel.start();
                spawn_missing_relays(
                    ctx.relay_controls,
                    ctx.relay_tx,
                    ctx.kernel,
                    ctx.next_relay_generation,
                );
            }
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
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
        ActorCommand::Shutdown => {
            close_relays(ctx.relay_controls, ctx.connected_relays, ctx.kernel);
            ctx.connected_urls.clear();
            None
        }
        ActorCommand::Protocol(cmd) => {
            // Step 1.b — the open-seam dispatch arm. Step 4 (V-41) widened
            // the context with the kernel + identity accessors the LNURL
            // fetcher needs (now_secs, author_write_relays, …); V-39+V-40
            // widens it further with the NIP-17 DM-stack surface (the
            // local-keys snapshot, DM-inbox relay lookup, last-error toast
            // writer, action-failure recorder). Each capability closure
            // closes over the actor's mutable references to the kernel +
            // identity runtime; the kernel and identity types stay
            // crate-private (D0 — NIP crates name neither). Borrows are
            // released the moment `cmd.run` returns — the worker thread the
            // LNURL command spawns owns its own `Sender<ActorCommand>`
            // clone and never re-enters the context.
            //
            // D6: a poisoned `nip17_local_keys` slot degrades to `None`
            // (same fail-closed behaviour the production DM inbox
            // projection uses — see `nmp_nip17::DmInboxProjection`).
            let tx = ctx.command_tx_self.clone();
            let send = move |c: crate::actor::ActorCommand| {
                // D6 — disconnected sender (post-Shutdown) is a benign
                // send-failure on the worker side; swallow as a no-op.
                let _ = tx.send(c);
            };
            // V-39+V-40 — read the NIP-17 local-keys slot once and snapshot
            // the DM-inbox lookup Arc. The lookup Arc lives for the
            // duration of this dispatch arm; the borrow into the context
            // is released before `emit_now`.
            let local_keys = ctx
                .nip17_local_keys
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().cloned());
            let dm_lookup = ctx.kernel.dm_inbox_relays_arc();
            // The kernel + identity accessors borrow disjoint fields of
            // `ctx`, so we hold the &mut Kernel for write methods
            // (`record_action_stage_requested`, `set_last_error_toast`,
            // `record_action_failure`) and immutable shared references to
            // the read-only kernel + identity sources via a `RefCell`
            // split. Rust's borrow checker permits multiple
            // `&dyn Fn(...)` over the same `&mut Kernel` so long as no two
            // are in flight simultaneously — `ProtocolCommand::run` is
            // single-threaded sync, so the calls serialize naturally.
            let kernel_cell = std::cell::RefCell::new(&mut *ctx.kernel);
            let identity_cell = std::cell::RefCell::new(&*ctx.identity);
            let now_secs = || kernel_cell.borrow().now_secs();
            let author_write_relays =
                |a: &str| kernel_cell.borrow().author_write_relays(a);
            let bootstrap_discovery_relays =
                || kernel_cell.borrow().bootstrap_discovery_relays();
            let active_local_keys = || {
                identity_cell
                    .borrow()
                    .active_local_keys()
                    .cloned()
            };
            let record_action_stage_requested = |cid: &str| {
                kernel_cell.borrow_mut().record_action_stage(
                    cid,
                    crate::kernel::action_stages::ActionStage::Requested,
                    None,
                );
            };
            // V-39+V-40 — D6 error-surface closures.
            let toast = |t: Option<String>| {
                if let Ok(mut k) = kernel_cell.try_borrow_mut() {
                    k.set_last_error_toast(t);
                }
            };
            let record_failure = |cid: String, err: String| {
                if let Ok(mut k) = kernel_cell.try_borrow_mut() {
                    k.record_action_failure(cid, err);
                }
            };
            // A second sender clone for the worker-thread surface. Cloning
            // a `mpsc::Sender` is cheap (atomic ref-count bump); the
            // dispatch arm always populates this slot in production.
            let worker_tx = ctx.command_tx_self.clone();
            let mut pctx = crate::substrate::ProtocolCommandContext::new(
                &send,
                worker_tx,
                &now_secs,
                &author_write_relays,
                &bootstrap_discovery_relays,
                &active_local_keys,
                &record_action_stage_requested,
                local_keys,
                &*dm_lookup,
                &toast,
                &record_failure,
            );
            if let Err(e) = cmd.run(&mut pctx) {
                tracing::warn!(error = %e, "ProtocolCommand returned error");
            }
            // Drop the context (and the closures it borrows) before the
            // emit so `emit_now` can re-borrow `ctx.kernel` mutably. The
            // closures themselves are zero-sized `Fn` closures (no `drop`
            // glue); only releasing the `RefCell` borrows the closures
            // hold via the context object matters here.
            drop(pctx);
            drop(kernel_cell);
            drop(identity_cell);
            // V-41 + V-39+V-40 — a `ProtocolCommand` body may have mutated
            // the kernel (the `Requested` stage write, a toast, a recorded
            // failure) or queued follow-up `ActorCommand`s
            // (`ShowToast` / `RecordActionFailure` / `PublishSignedEvent`).
            // Emit promptly so the next snapshot tick carries the visible
            // effect, mirroring the legacy `FetchLnurlInvoice` and
            // `SendGiftWrappedDm` arms' `emit_now` precedents.
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
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

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_relay_event(
    event: RelayEvent,
    kernel: &mut Kernel,
    #[cfg(feature = "wallet")] wallet: &mut WalletRuntime,
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    next_relay_generation: &mut u64,
    connected_relays: &mut HashSet<RelayRole>,
    connected_urls: &mut HashSet<CanonicalRelayUrl>,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
    startup_sent: &mut bool,
    running: bool,
) {
    match event {
        RelayEvent::Connected {
            role, relay_url, ..
        } => {
            connected_relays.insert(role);
            kernel.relay_connected(role);
            // T116/G1 — reconnect-replay. The first `Connected` for a URL is
            // the initial dial; the startup path (`maybe_send_startup` /
            // `kernel.startup_requests()`) emits REQs there. Every
            // subsequent `Connected` after a `Failed`/`Closed` is a true
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
            let is_reconnect = !connected_urls.insert(CanonicalRelayUrl::parse_or_raw(&relay_url));
            if is_reconnect && running {
                let replay = kernel.replay_on_reconnect(role, &relay_url);
                if !replay.is_empty() {
                    send_all_outbound(
                        relay_controls,
                        relay_tx,
                        kernel,
                        next_relay_generation,
                        replay,
                    );
                }
            }
            if running {
                let publish_replay = kernel.mark_publish_relay_available(&relay_url);
                if !publish_replay.is_empty() {
                    send_all_outbound(
                        relay_controls,
                        relay_tx,
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
                relay_tx,
                kernel,
                next_relay_generation,
            );
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Failed {
            role,
            relay_url,
            error,
            ..
        } => {
            connected_relays.remove(&role);
            *startup_sent = false;
            // T105: scope the `retrying` mark to the specific socket that
            // failed — sibling sockets sharing this role lane are still live.
            kernel.relay_failed(role, &relay_url, error);
            kernel.mark_publish_relay_unavailable(&relay_url);
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Closed {
            role, relay_url, ..
        } => {
            connected_relays.remove(&role);
            *startup_sent = false;
            // T105: scope T133 wire-sub eviction to the closed socket's URL,
            // not the whole role lane (sibling sockets keep their subs).
            kernel.relay_closed(role, &relay_url);
            kernel.mark_publish_relay_unavailable(&relay_url);
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Message {
            role,
            relay_url,
            message,
            ..
        } if running => {
            // NWC relay intercept: peek at text frames from the wallet relay
            // for kind:23195 responses before passing to kernel.handle_message.
            // The kernel silently drops unknown kinds, so letting it see wallet
            // events too is harmless; we just need to decrypt them first.
            // D0: gated behind the `wallet` feature — NWC is an app noun.
            #[cfg(feature = "wallet")]
            let wallet_text = if wallet.is_nwc_relay(&relay_url) {
                match &message {
                    tungstenite::Message::Text(s) => Some(s.clone()),
                    _ => None,
                }
            } else {
                None
            };
            // V-01 Phase 1c: convert the native `tungstenite::Message` into the
            // wire-transport-agnostic [`RelayFrame`] before crossing the kernel
            // boundary. The kernel no longer names `tungstenite`; the conversion
            // lives here (the only native call site) and at the relay-worker
            // socket-read seam.
            let frame = tungstenite_message_to_relay_frame(message);
            let mut outbound = kernel.handle_message(role, &relay_url, frame);
            outbound.extend(kernel.pending_view_requests());
            #[cfg(feature = "wallet")]
            if let Some(text) = wallet_text {
                let wallet_out = commands::handle_nwc_text(wallet, &text, kernel);
                outbound.extend(wallet_out);
            }
            send_all_outbound(
                relay_controls,
                relay_tx,
                kernel,
                next_relay_generation,
                outbound,
            );
        }
        RelayEvent::Message { .. } => {}
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
        add_relay, new_bunker_handshake_slot, remove_relay, sign_in_nsec, IdentityRuntime,
    };
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    /// Throwaway nsec — generated for tests only, never on the network.
    /// Same dev key the conformance harness round-trip tests
    /// (`tests/nip_tag_conformance.rs`) and the remote-signer tests
    /// (`actor/commands/remote_signer_tests.rs`) use. Reusing it here
    /// keeps the test fixture surface small.
    const TEST_NSEC: &str =
        "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

    fn fresh_kernel() -> Kernel {
        Kernel::new(DEFAULT_VISIBLE_LIMIT)
    }

    fn fresh_identity() -> IdentityRuntime {
        IdentityRuntime::new(new_bunker_handshake_slot())
    }

    fn signed_in_identity(kernel: &mut Kernel) -> IdentityRuntime {
        let mut identity = fresh_identity();
        sign_in_nsec(&mut identity, kernel, TEST_NSEC, false);
        assert!(
            identity.active_pubkey().is_some(),
            "sign_in_nsec must produce an active account",
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
        let before = kernel.relay_edit_rows_snapshot().to_vec();
        let added = add_relay(&mut kernel, "wss://relay.example", "both");
        assert!(added.is_some(), "add_relay must accept a valid wss:// URL");

        let outbound = maybe_publish_relay_list_after_edit(
            &mut identity,
            &mut kernel,
            &before,
            &mut pending,
        );
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

        let before = kernel.relay_edit_rows_snapshot().to_vec();
        add_relay(&mut kernel, "wss://relay.example", "both");

        let outbound = maybe_publish_relay_list_after_edit(
            &mut identity,
            &mut kernel,
            &before,
            &mut pending,
        );
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
        let before = kernel.relay_edit_rows_snapshot().to_vec();
        add_relay(&mut kernel, "wss://relay.example", "both");

        let outbound = maybe_publish_relay_list_after_edit(
            &mut identity,
            &mut kernel,
            &before,
            &mut pending,
        );
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

        let before = kernel.relay_edit_rows_snapshot().to_vec();
        remove_relay(&mut kernel, "wss://other.example");

        let outbound = maybe_publish_relay_list_after_edit(
            &mut identity,
            &mut kernel,
            &before,
            &mut pending,
        );
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

        let before = kernel.relay_edit_rows_snapshot().to_vec();
        remove_relay(&mut kernel, "wss://drop.example");

        let outbound = maybe_publish_relay_list_after_edit(
            &mut identity,
            &mut kernel,
            &before,
            &mut pending,
        );
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

        let before = kernel.relay_edit_rows_snapshot().to_vec();
        remove_relay(&mut kernel, "wss://only.example");
        assert!(
            kernel.relay_edit_rows_snapshot().is_empty(),
            "test precondition: projection must be empty after removing the only row"
        );

        let outbound = maybe_publish_relay_list_after_edit(
            &mut identity,
            &mut kernel,
            &before,
            &mut pending,
        );
        assert_eq!(
            count_kind_10002_frames(&outbound),
            0,
            "removing the user's last NIP-65-eligible row MUST NOT \
             publish an empty kind:10002 (that would clear the \
             author_relay_lists cache on ingest — destructive)",
        );
    }
}
