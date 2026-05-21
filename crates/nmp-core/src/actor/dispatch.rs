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

use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole};
use crate::relay_worker::RelayEvent;

use super::commands::{self, IdentityRuntime, LifecycleObserverSlot};
// D0: NIP-47 NWC is an app noun — `WalletRuntime` only exists with `wallet`.
#[cfg(feature = "wallet")]
use super::commands::WalletRuntime;
use super::kernel_action::dispatch_kernel_action;
use super::pending_sign::PendingSign;
use super::relay_mgmt::{
    close_relays, ensure_relay_worker, maybe_send_startup, send_all_outbound,
    shutdown_relay_worker, spawn_missing_relays,
};
use super::session_persistence;
use super::tick::{emit_kernel_update, emit_now, maybe_emit_after_dispatch};
use super::{ActorCommand, RelayControl};
use crate::capability_socket::CapabilityCallbackSlot;

/// Write the active account's bech32 secret key (or `None`) to `slot`.
/// Called synchronously BEFORE `maybe_emit_after_dispatch` so the value is
/// visible before Swift's `apply()` runs.
///
/// The bech32 secret is wrapped in [`Zeroizing`] so the previous value is
/// wiped from the heap when this overwrite drops it.
fn update_nsec_slot(identity: &IdentityRuntime, slot: &Arc<Mutex<Option<Zeroizing<String>>>>) {
    if let Ok(mut guard) = slot.lock() {
        *guard = identity.active_nsec_bech32().map(Zeroizing::new);
    }
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
    pub(super) active_local_nsec: &'a Arc<Mutex<Option<Zeroizing<String>>>>,
    pub(super) capability_callback: &'a CapabilityCallbackSlot,
    pub(super) pending_signs: &'a mut Vec<PendingSign>,
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
            ctx.kernel.start();
            let mut outbound = session_persistence::restore_active_session(
                ctx.identity,
                ctx.kernel,
                ctx.capability_callback,
                ctx.relays_ready,
            );
            update_nsec_slot(ctx.identity, ctx.active_local_nsec);
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
            let outbound = commands::sign_in_nsec(
                ctx.identity,
                ctx.kernel,
                secret.as_str(),
                ctx.relays_ready,
            );
            update_nsec_slot(ctx.identity, ctx.active_local_nsec);
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
            update_nsec_slot(ctx.identity, ctx.active_local_nsec);
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
            update_nsec_slot(ctx.identity, ctx.active_local_nsec);
            session_persistence::persist_current_active_session(
                ctx.identity,
                ctx.capability_callback,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::RemoveAccount { identity_id } => {
            let outbound = commands::remove_account(ctx.identity, ctx.kernel, &identity_id);
            update_nsec_slot(ctx.identity, ctx.active_local_nsec);
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
            update_nsec_slot(ctx.identity, ctx.active_local_nsec);
            session_persistence::persist_current_active_session(
                ctx.identity,
                ctx.capability_callback,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::RemoveRemoteSigner { identity_id } => {
            let outbound = commands::remove_remote_signer(ctx.identity, ctx.kernel, &identity_id);
            update_nsec_slot(ctx.identity, ctx.active_local_nsec);
            session_persistence::forget_account(&identity_id, ctx.capability_callback);
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
            correlation_id,
        } => {
            let outbound = commands::publish_note(
                ctx.identity,
                ctx.kernel,
                &content,
                reply_to_id.as_deref(),
                correlation_id,
                ctx.pending_signs,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishProfile {
            fields,
            correlation_id,
        } => {
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
        ActorCommand::PublishUnsignedEvent(mut unsigned) => {
            // D7: apply the same created_at=0 sentinel as PublishUnsignedEventToRelays.
            // A host that builds an UnsignedEvent without setting created_at gets
            // the kernel clock rather than epoch time.
            if unsigned.created_at == 0 {
                unsigned.created_at = ctx.kernel.now_secs();
            }
            let outbound = commands::publish_unsigned_event(
                ctx.identity,
                ctx.kernel,
                unsigned,
                ctx.pending_signs,
            );
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishUnsignedEventToRelays { mut event, relays } => {
            // D7: kernel owns the wall clock. Executors in NIP crates set
            // created_at = 0 as a sentinel; we re-stamp here so they never
            // call SystemTime::now() and the FixedClock test hook stays
            // effective end-to-end.
            if event.created_at == 0 {
                event.created_at = ctx.kernel.now_secs();
            }
            let outbound = commands::publish_unsigned_event_to_relays(
                ctx.identity,
                ctx.kernel,
                event,
                relays,
                ctx.pending_signs,
            );
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::PublishSignedEvent { raw, relays } => {
            let outbound = commands::publish_signed_event(ctx.kernel, raw, &relays);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::SendGiftWrappedDm {
            rumor,
            recipient_pubkey,
        } => {
            // NIP-17: seal + gift-wrap the kind:14 rumor into two kind:1059
            // envelopes (recipient + self-copy) and publish them. The gift-wrap
            // crypto runs here on the actor thread (D7). `created_at == 0` is
            // re-stamped from the kernel clock inside the handler.
            let outbound =
                commands::send_gift_wrapped_dm(ctx.identity, ctx.kernel, rumor, &recipient_pubkey);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
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
        } => {
            let outbound = commands::react(
                ctx.identity,
                ctx.kernel,
                &target_event_id,
                &reaction,
                ctx.pending_signs,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::Follow { pubkey } => {
            let outbound =
                commands::follow(ctx.identity, ctx.kernel, &pubkey, true, ctx.pending_signs);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::Unfollow { pubkey } => {
            let outbound =
                commands::follow(ctx.identity, ctx.kernel, &pubkey, false, ctx.pending_signs);
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
            if let Some(canonical_url) = commands::add_relay(ctx.kernel, &url, &role) {
                ensure_relay_worker(
                    ctx.relay_controls,
                    ctx.relay_tx,
                    ctx.kernel,
                    ctx.next_relay_generation,
                    crate::relay::RelayRole::Content,
                    canonical_url,
                );
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
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
            shutdown_relay_worker(ctx.relay_controls, &url);
            commands::remove_relay(ctx.kernel, &url);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
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
        } => {
            let outbound =
                commands::wallet_pay_invoice(ctx.wallet, ctx.kernel, &bolt11, amount_msats);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
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
            let update = dispatch_kernel_action(ctx.kernel, action);
            // Discrete FFI update: emit as the tagged `{"t":"update","v":…}`
            // envelope so consumers decode the single `UpdateEnvelope` type
            // (D6 — the tag is the discriminant, no key sniffing).
            emit_kernel_update(&update, ctx.update_tx);
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
            let raw_event_observers_handle =
                ctx.kernel.take_raw_event_observers_handle_for_reset();
            // Preserve the snapshot-projection slot across Reset for the same
            // reason: the `Arc<Mutex<…>>` is shared with the FFI surface and
            // per-app crates; replacing it would silently drop every
            // host-registered projection from the snapshot.
            let snapshot_projection_handle =
                ctx.kernel.take_snapshot_projection_handle_for_reset();
            // Preserve the relay-edit rows handle across Reset for the same
            // reason: the `Arc<Mutex<…>>` is shared with the FFI surface
            // and per-app crates; replacing it would silently return stale
            // rows to Marmot dispatch.
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
        ActorCommand::Shutdown => {
            close_relays(ctx.relay_controls, ctx.connected_relays, ctx.kernel);
            ctx.connected_urls.clear();
            None
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
            let mut outbound = kernel.handle_message(role, &relay_url, message);
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
