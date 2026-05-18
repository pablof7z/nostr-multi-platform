//! Command + relay-event dispatch reducers.
//!
//! Split out of `mod.rs` to keep both files under the 300-LOC soft cap.
//! `dispatch_command` resolves an [`ActorCommand`] into outbound relay
//! messages (or `None` for shutdown); `handle_relay_event` folds a
//! [`RelayEvent`] into the kernel + connection bookkeeping. No behavior
//! change — pure move of the two reducers off the actor loop.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole};
use crate::relay_worker::RelayEvent;

use super::commands::{self, IdentityRuntime, LifecycleObserverSlot, WalletRuntime};
use super::kernel_action::dispatch_kernel_action;
use super::relay_mgmt::{
    close_relays, ensure_relay_worker, maybe_send_startup, send_all_outbound,
    shutdown_relay_worker, spawn_missing_relays,
};
use super::tick::{emit_kernel_update, emit_now, maybe_emit_after_dispatch};
use super::{ActorCommand, RelayControl};

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_command(
    command: ActorCommand,
    kernel: &mut Kernel,
    identity: &mut IdentityRuntime,
    wallet: &mut WalletRuntime,
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    connected_relays: &mut HashSet<RelayRole>,
    connected_urls: &mut HashSet<String>,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
    next_relay_generation: &mut u64,
    running: &mut bool,
    emit_hz: &mut u32,
    startup_sent: &mut bool,
    relays_ready: bool,
    lifecycle_observer: &LifecycleObserverSlot,
) -> Option<Vec<OutboundMessage>> {
    match command {
        ActorCommand::Start {
            visible_limit,
            emit_hz: hz,
        } => {
            *running = true;
            *emit_hz = hz;
            *startup_sent = false;
            kernel.set_visible_limit(visible_limit);
            kernel.start();
            spawn_missing_relays(relay_controls, relay_tx, kernel, next_relay_generation);
            emit_now(kernel, *running, update_tx, last_emit);
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
            Some(kernel.resume_publish_engine())
        }
        ActorCommand::Configure {
            visible_limit,
            emit_hz: hz,
        } => {
            *emit_hz = hz;
            kernel.set_visible_limit(visible_limit);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::OpenAuthor { pubkey } => {
            let outbound = kernel.open_author(pubkey, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::OpenThread { event_id } => {
            let outbound = kernel.open_thread(event_id, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::OpenFirehoseTag { tag } => {
            let outbound = kernel.open_firehose_tag(tag, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::ClaimProfile {
            pubkey,
            consumer_id,
        } => {
            let outbound = kernel.claim_profile(pubkey, consumer_id, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::ReleaseProfile {
            pubkey,
            consumer_id,
        } => {
            let outbound = kernel.release_profile(&pubkey, &consumer_id);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::CloseAuthor { pubkey } => {
            let outbound = kernel.close_author(&pubkey);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::CloseThread { event_id } => {
            let outbound = kernel.close_thread(&event_id);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::SignInNsec { secret } => {
            let outbound = commands::sign_in_nsec(identity, kernel, &secret, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::SignInBunker { uri } => {
            commands::sign_in_bunker(kernel, &uri);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::CreateAccount => {
            let outbound = commands::create_account(identity, kernel, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::SwitchActive { identity_id } => {
            let outbound =
                commands::switch_active(identity, kernel, &identity_id, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::RemoveAccount { identity_id } => {
            let outbound = commands::remove_account(identity, kernel, &identity_id);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::AddRemoteSigner { handle } => {
            let outbound =
                commands::add_remote_signer(identity, kernel, handle, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::RemoveRemoteSigner { identity_id } => {
            let outbound = commands::remove_remote_signer(identity, kernel, &identity_id);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::BunkerHandshakeProgress { stage, message } => {
            commands::bunker_handshake_progress(kernel, stage, message);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::PublishNote {
            content,
            reply_to_id,
        } => {
            let outbound =
                commands::publish_note(identity, kernel, &content, reply_to_id.as_deref());
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::PublishUnsignedEvent(unsigned) => {
            let outbound = commands::publish_unsigned_event(identity, kernel, unsigned);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::PublishSignedEvent { raw, relays } => {
            let outbound = commands::publish_signed_event(kernel, raw, &relays);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::React {
            target_event_id,
            reaction,
        } => {
            let outbound =
                commands::react(identity, kernel, &target_event_id, &reaction);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::Follow { pubkey } => {
            let outbound = commands::follow(identity, kernel, &pubkey, true);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::Unfollow { pubkey } => {
            let outbound = commands::follow(identity, kernel, &pubkey, false);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::AddRelay { url, role } => {
            // T158: add_relay now returns Some(canonical_url) on success so we
            // can dial a real socket immediately. User-added relays use
            // RelayRole::Content as the diagnostic lane (inbox/outbox bucket);
            // the NIP-65 read/write distinction lives in RelayEditRow, not in
            // the transport pool key (T105). ensure_relay_worker is idempotent —
            // a role-edit for an already-connected URL is a harmless no-op.
            if let Some(canonical_url) = commands::add_relay(kernel, &url, &role) {
                ensure_relay_worker(
                    relay_controls,
                    relay_tx,
                    kernel,
                    next_relay_generation,
                    crate::relay::RelayRole::Content,
                    canonical_url,
                );
            }
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
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
            shutdown_relay_worker(relay_controls, &url);
            commands::remove_relay(kernel, &url);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::OpenTimeline => {
            let outbound = commands::open_timeline(identity, kernel, relays_ready);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::WalletConnect { uri } => {
            let outbound = commands::wallet_connect(wallet, kernel, &uri);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::WalletDisconnect => {
            let outbound = commands::wallet_disconnect(wallet, kernel);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::WalletPayInvoice { bolt11, amount_msats } => {
            let outbound = commands::wallet_pay_invoice(wallet, kernel, &bolt11, amount_msats);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::LifecycleEvent(phase) => {
            // T118 / G3 — fold scenePhase into the kernel state and fire
            // the registered observer (if any) on a meaningful transition.
            // The handler is idempotent (rapid scene oscillation collapses
            // to a single observer call) and never emits outbound frames;
            // the consumer's TriggerEngine drives any reconcile work
            // through its own path on the next tick.
            commands::handle_lifecycle_event(kernel, lifecycle_observer, phase);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Kernel(action) => {
            let update = dispatch_kernel_action(kernel, action);
            // Discrete FFI update: emit as the tagged `{"t":"update","v":…}`
            // envelope so consumers decode the single `UpdateEnvelope` type
            // (D6 — the tag is the discriminant, no key sniffing).
            emit_kernel_update(&update, update_tx);
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::ShowToast { message } => {
            // D6 — FFI-boundary validation errors reach the kernel as state
            // via this command. The FFI layer only has a channel sender; this
            // arm is the single path from the FFI to `set_last_error_toast`.
            kernel.set_last_error_toast(Some(message));
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Stop => {
            *running = false;
            *startup_sent = false;
            close_relays(relay_controls, connected_relays, kernel);
            // T116/G1 — clear reconnect-replay discriminator so a subsequent
            // Start replays cleanly (every URL appears as a first-connect).
            connected_urls.clear();
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Reset => {
            close_relays(relay_controls, connected_relays, kernel);
            connected_urls.clear();
            // T114b — preserve the FFI-channel drop-counter handle across
            // Reset (the underlying Arc<AtomicU64> is shared with the FFI
            // forwarder thread and must NOT be replaced; the counter is
            // process-lifetime).
            let drops_handle = kernel.take_dispatch_drops_handle_for_reset();
            // T146 — preserve the event observer slot across Reset for the
            // same reason: the `Arc<Mutex<…>>` is shared with the FFI
            // surface and per-app crates; replacing it would silently
            // disconnect every registered observer.
            let event_observers_handle = kernel.take_event_observers_handle_for_reset();
            // Preserve the raw signed-event tap slot across Reset for the
            // same reason: the `Arc<Mutex<…>>` is shared with the FFI
            // surface and per-app crates; replacing it would silently
            // disconnect every registered raw observer.
            let raw_event_observers_handle =
                kernel.take_raw_event_observers_handle_for_reset();
            *kernel = Kernel::new(kernel.visible_limit());
            if let Some(handle) = drops_handle {
                kernel.set_dispatch_drops_handle(handle);
            }
            if let Some(handle) = event_observers_handle {
                kernel.set_event_observers_handle(handle);
            }
            if let Some(handle) = raw_event_observers_handle {
                kernel.set_raw_event_observers_handle(handle);
            }
            *startup_sent = false;
            if *running {
                kernel.start();
                spawn_missing_relays(relay_controls, relay_tx, kernel, next_relay_generation);
            }
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Shutdown => {
            close_relays(relay_controls, connected_relays, kernel);
            connected_urls.clear();
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
                kernel.ingest_pre_verified_event(
                    crate::relay::RelayRole::Content,
                    "diag-firehose-stress",
                    verified,
                );
            }
            // One sort after all events are ingested: O(n log n) not O(n²·log n).
            kernel.sort_timeline_deferred();
            maybe_emit_after_dispatch(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_relay_event(
    event: RelayEvent,
    kernel: &mut Kernel,
    wallet: &mut WalletRuntime,
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    next_relay_generation: &mut u64,
    connected_relays: &mut HashSet<RelayRole>,
    connected_urls: &mut HashSet<String>,
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
            let is_reconnect = !connected_urls.insert(relay_url.clone());
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
        RelayEvent::Failed { role, error, .. } => {
            connected_relays.remove(&role);
            *startup_sent = false;
            kernel.relay_failed(role, error);
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Closed { role, .. } => {
            connected_relays.remove(&role);
            *startup_sent = false;
            kernel.relay_closed(role);
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
