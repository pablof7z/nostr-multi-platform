//! Command + relay-event dispatch reducers.
//!
//! Split out of `actor/mod.rs` to keep both files under the LOC ceiling.
//! `dispatch_command` resolves an [`ActorCommand`] into outbound relay
//! messages (or `None` for shutdown); `handle_relay_event` folds a
//! [`nmp_network::pool::PoolEvent`] into the kernel + connection bookkeeping.
//!
//! ## Sub-module layout
//!
//! | File | Contents |
//! |------|----------|
//! | `mod.rs` | `ActorContext`, `build_open_interest`, thin `dispatch_command` delegator |
//! | `cmd_lifecycle.rs` | `Start`, `Stop`, `Reset`, `Shutdown` arms |
//! | `cmd_identity.rs` | AddSigner / CreateAccount / SwitchActive / … / SignEventForReturn |
//! | `cmd_publish.rs` | Publish / follow / relay-mutation / action-record arms |
//! | `cmd_interests.rs` | Interest, pull-cursor, and test-support ingest/GC arms |
//! | `cmd_protocol.rs` | `Protocol(cmd)` arm with catch-unwind + RefCell adapters |
//! | `relay_events.rs` | `handle_relay_event` + `resolve_handle` |
//! | `helpers.rs` | `update_local_key_slots`, `maybe_publish_relay_list_after_edit`, … |
//! | `substrate_adapters.rs` | Capability adapters for `ProtocolCommandContext` |
//! | `open_interest_tests.rs` | `OpenInterest` / `CloseInterest` kernel-side tests |
//! | `nip65_tests.rs` | NIP-65 auto-publish end-to-end tests |

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use nmp_network::pool::Pool;

use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole};
use crate::slots::{ActiveLocalKeysSlot, MlsLocalNsecSlot};

use super::capability_worker::CapabilityWorkSender;
use super::commands::{self, IdentityRuntime, LifecycleObserverSlot};
use super::pending_sign::ParkedSignerOps;
use super::signer_port_dispatch;
use super::tick::maybe_emit_after_dispatch;
use super::{ActorCommand, ActorConfig, RelayControl};
use crate::capability_socket::CapabilityCallbackSlot;
use crate::kernel_action::dispatch_kernel_action;

// Sub-modules — each covers one logical slice of the dispatch surface.
mod cmd_identity;
mod cmd_interests;
mod cmd_lifecycle;
mod cmd_protocol;
mod cmd_publish;
mod helpers;
mod relay_events;
// Debt C — capability adapters for `ProtocolCommandContext`.
mod substrate_adapters;

// Re-exports needed by callers outside this module.
pub(crate) use helpers::signed_event_to_json;
pub(crate) use relay_events::handle_relay_event;

#[cfg(test)]
mod nip65_tests;
#[cfg(test)]
mod open_interest_tests;

/// M2 (ADR-0042) — thin shim delegating to the always-compiled
/// [`crate::subs::interest_builder::build_interest_pair`].
pub(crate) fn build_open_interest(
    filter_json: &str,
    consumer_id: &str,
    scope: u32,
    relay_pin: Option<&str>,
) -> Option<(crate::subs::SubIdentity, crate::planner::LogicalInterest)> {
    crate::subs::interest_builder::build_interest_pair(filter_json, consumer_id, scope, relay_pin)
}

/// Borrowed bundle of the actor loop's mutable runtime state.
///
/// Replaces the 15+ explicit parameters that `dispatch_command` used to take.
/// Constructed fresh per command in `run_actor_with_observers` and dropped
/// immediately after dispatch, so every other call site in the actor loop
/// keeps using the original locals untouched. The lifetime `'a` ties the
/// struct to those stack-resident locals — no heap allocation, no ownership
/// transfer, the actor loop still owns every field.
pub(super) struct ActorContext<'a> {
    pub(super) kernel: &'a mut Kernel,
    pub(super) identity: &'a mut IdentityRuntime,
    pub(super) relay_controls: &'a mut HashMap<CanonicalRelayUrl, RelayControl>,
    /// slot() → canonical URL reverse-map for O(1) `PoolEvent` resolution.
    pub(super) slot_to_url: &'a mut HashMap<u32, CanonicalRelayUrl>,
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
    /// Active-account `nostr::Keys` slot; written with `mls_local_nsec` on every identity mutation.
    pub(super) active_local_keys: &'a ActiveLocalKeysSlot,
    pub(super) capability_callback: &'a CapabilityCallbackSlot,
    /// Unified parked-op queue (ADR-0050 §D2).
    pub(super) parked_ops: &'a mut ParkedSignerOps,
    /// Actor's own waking inbox sender (ADR-0050 §D3a). D8 — only cloned out, never recv'd.
    pub(super) command_tx_self: &'a crate::actor::CommandSender,
    /// Capability-worker queue sender (ADR-0040 §3 / V-90). D8 — only sends.
    pub(super) capability_work_tx: &'a CapabilityWorkSender,
    /// Snapshotted actor setup; Reset re-applies the same immutable view.
    pub(super) config: &'a ActorConfig,
    /// V-51 routing-trace slot — re-published on Reset.
    pub(super) routing_trace_slot:
        &'a Arc<Mutex<Option<Arc<crate::kernel::routing_trace::RoutingTraceProjection>>>>,
    /// V-83 event-store slot — re-published on Reset.
    pub(super) event_store_slot: &'a crate::slots::EventStoreSlot,
    /// ADR-0058 pull-cursor registry slot — re-published on Reset.
    pub(super) pull_cursor_registry_slot: &'a crate::slots::PullCursorRegistryHandleSlot,
    /// V-82 FFI-shared active-account slot — re-bound on Reset.
    pub(super) active_account_slot: &'a crate::slots::ActiveAccountSlot,
    /// External event sink dispatcher — re-registered on Reset.
    pub(super) external_event_sink_dispatcher: &'a crate::substrate::ExternalEventSinkDispatcher,
}

pub(super) fn dispatch_command(
    command: ActorCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match command {
        ActorCommand::Start {
            visible_limit,
            emit_hz: requested_hz,
            initial_relays,
        } => cmd_lifecycle::start(visible_limit, requested_hz, initial_relays, ctx),
        ActorCommand::Configure {
            visible_limit,
            emit_hz: requested_hz,
        } => {
            use crate::actor::tick::{clamp_emit_hz_logged, emit_now};
            *ctx.emit_hz = clamp_emit_hz_logged(ctx.kernel, requested_hz, "Configure");
            ctx.kernel.set_visible_limit(visible_limit);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::ClaimEvent { uri, consumer_id, force } => {
            let outbound = ctx.kernel.claim_event(uri, consumer_id, ctx.relays_ready, force);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::ReleaseEvent { uri, consumer_id } => {
            let outbound = ctx.kernel.release_event(&uri, &consumer_id);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::ResolveRef { namespace, key, consumer_id, shape, liveness, force, hints } => {
            let outbound = ctx.kernel.resolve_ref(namespace, key, consumer_id, shape, liveness, force, hints);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::ReleaseRef { namespace, key, consumer_id } => {
            let outbound = ctx.kernel.release_ref(namespace, &key, &consumer_id);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        ActorCommand::SignEventForReturn { account_pubkey, unsigned_json, correlation_id } =>
            cmd_identity::sign_event_for_return(account_pubkey, unsigned_json, correlation_id, ctx),
        ActorCommand::SignEventForAccount { unsigned, signer_pubkey, continuation } =>
            signer_port_dispatch::sign_for_account(ctx, &unsigned, signer_pubkey, continuation),
        ActorCommand::Nip44EncryptForAccount { peer_pubkey, plaintext, signer_pubkey, continuation } =>
            signer_port_dispatch::nip44_encrypt_for_account(ctx, &peer_pubkey, &plaintext, signer_pubkey, continuation),
        ActorCommand::Nip44DecryptForAccount { peer_pubkey, ciphertext, signer_pubkey, continuation } =>
            signer_port_dispatch::nip44_decrypt_for_account(ctx, &peer_pubkey, &ciphertext, signer_pubkey, continuation),
        ActorCommand::DeliverSignerResponse { response_json } =>
            signer_port_dispatch::deliver_signer_response(ctx, &response_json),
        ActorCommand::AddSigner { source, make_active } =>
            cmd_identity::add_signer(source, make_active, ctx),
        ActorCommand::CreateAccount { profile, relays, initial_follows, mls, make_active } =>
            cmd_identity::create_account(profile, relays, initial_follows, mls, make_active, ctx),
        ActorCommand::SwitchActive { identity_id } =>
            cmd_identity::switch_active(identity_id, ctx),
        ActorCommand::RemoveAccount { identity_id } =>
            cmd_identity::remove_account(identity_id, ctx),
        ActorCommand::BunkerHandshakeProgress { stage, code, message } =>
            cmd_identity::bunker_handshake_progress(stage, code, message, ctx),
        ActorCommand::BunkerConnectionStateChanged { state, reason } =>
            cmd_identity::bunker_connection_state_changed(state, reason, ctx),
        ActorCommand::Nip55SignerStateChanged { state, reason } =>
            cmd_identity::nip55_signer_state_changed(state, reason, ctx),
        ActorCommand::PublishRawEvent { kind, tags, content, target, signer_pubkey, correlation_id } =>
            cmd_publish::publish_raw_event(kind, tags, content, target, signer_pubkey, correlation_id, ctx),
        ActorCommand::PublishProfile { fields, correlation_id } =>
            cmd_publish::publish_profile(fields, correlation_id, ctx),
        ActorCommand::PublishUnsignedEvent { event: unsigned, correlation_id, signer_pubkey } =>
            cmd_publish::publish_unsigned_event(unsigned, correlation_id, signer_pubkey, ctx),
        ActorCommand::PublishUnsignedEventToRelays { event, relays, correlation_id, signer_pubkey } =>
            cmd_publish::publish_unsigned_event_to_relays(event, relays, correlation_id, signer_pubkey, ctx),
        ActorCommand::PublishSignedEvent { raw, target, correlation_id } =>
            cmd_publish::publish_signed_event(raw, target, correlation_id, ctx),
        // V-39: SendGiftWrappedDm deleted — now routes through Protocol.
        ActorCommand::RetryPublish { handle } =>
            cmd_publish::retry_publish(handle, ctx),
        ActorCommand::CancelPublish { correlation_id } =>
            cmd_publish::cancel_publish(correlation_id, ctx),
        ActorCommand::Follow { pubkey, correlation_id } =>
            cmd_publish::follow_or_unfollow(pubkey, true, correlation_id, ctx),
        ActorCommand::Unfollow { pubkey, correlation_id } =>
            cmd_publish::follow_or_unfollow(pubkey, false, correlation_id, ctx),
        ActorCommand::FollowMany { pubkeys, correlation_id } =>
            cmd_publish::follow_many(pubkeys, correlation_id, ctx),
        ActorCommand::AddRelay { url, role } => cmd_publish::add_relay(url, role, ctx),
        ActorCommand::RemoveRelay { url } => cmd_publish::remove_relay(url, ctx),
        ActorCommand::ReconnectRelays => cmd_publish::reconnect_relays_cmd(ctx),
        ActorCommand::DeclareActiveFollowsFeed { acquisition_kinds } =>
            cmd_publish::declare_active_follows_feed(acquisition_kinds, ctx),
        ActorCommand::ClearActiveFollowsFeed => cmd_publish::clear_active_follows_feed(ctx),
        // V-38/V-41: Wallet* and FetchLnurlInvoice deleted; route through Protocol.
        ActorCommand::RecordActionFailure { correlation_id, reason } =>
            cmd_publish::record_action_failure(correlation_id, reason, ctx),
        ActorCommand::SetRelayInfo { relay_url, doc_json } =>
            cmd_publish::set_relay_info(relay_url, doc_json, ctx),
        ActorCommand::RecordActionSuccess { correlation_id, result_json } =>
            cmd_publish::record_action_success(correlation_id, result_json, ctx),
        ActorCommand::AckActionStage(correlation_id) =>
            cmd_publish::ack_action_stage(correlation_id, ctx),
        ActorCommand::LifecycleEvent(phase) => {
            commands::handle_lifecycle_event(ctx.kernel, ctx.lifecycle_observer, phase);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::Kernel(action) => {
            let _ = dispatch_kernel_action(ctx.kernel, action);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::ShowToast { message } => {
            ctx.kernel.set_last_error_toast(Some(message));
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::ShowErrorToken { token } => {
            ctx.kernel.set_last_error_token(&token);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        ActorCommand::MarkChangedSinceEmit => {
            ctx.kernel.mark_changed_since_emit();
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        #[cfg(feature = "native")]
        ActorCommand::CapabilityResultReady { account_id, result_json } =>
            cmd_identity::capability_result_ready(account_id, result_json, ctx),
        ActorCommand::Stop => cmd_lifecycle::stop(ctx),
        ActorCommand::Reset => cmd_lifecycle::reset(ctx),
        ActorCommand::PushInterest(interest) => cmd_interests::push_interest(interest, ctx),
        ActorCommand::WithdrawInterest(id) => cmd_interests::withdraw_interest(id, ctx),
        ActorCommand::EnsureInterest { identity, interest } =>
            cmd_interests::ensure_interest(identity, interest, ctx),
        ActorCommand::DropInterestOwner(identity) =>
            cmd_interests::drop_interest_owner(identity, ctx),
        ActorCommand::RegisterPullCursor { cursor_id, consumer_id, scope, mode, after_seq, limits } =>
            cmd_interests::register_pull_cursor(cursor_id, consumer_id, scope, mode, after_seq, limits, ctx),
        ActorCommand::AdvancePullCursor { cursor_id, after_seq } =>
            cmd_interests::advance_pull_cursor(cursor_id, after_seq, ctx),
        ActorCommand::UnregisterPullCursor { cursor_id } =>
            cmd_interests::unregister_pull_cursor(cursor_id, ctx),
        ActorCommand::OpenInterest { filter_json, consumer_id, scope } =>
            cmd_interests::open_interest(filter_json, consumer_id, scope, ctx),
        ActorCommand::OpenObservedInterest {
            filter_json, consumer_id, scope, relay_pin,
            observer_id, replay_shapes, replay_limit,
        } => cmd_interests::open_observed_interest(
            filter_json, consumer_id, scope, relay_pin,
            observer_id, replay_shapes, replay_limit, ctx,
        ),
        ActorCommand::CloseInterest { filter_json, consumer_id, scope, relay_pin } =>
            cmd_interests::close_interest(filter_json, consumer_id, scope, relay_pin, ctx),
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::Barrier { ack } => { let _ = ack.send(()); Some(Vec::new()) }
        ActorCommand::Shutdown => cmd_lifecycle::shutdown(ctx),
        ActorCommand::Protocol(cmd) => cmd_protocol::protocol(cmd, ctx),
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::IngestPreVerifiedEvents(events) =>
            cmd_interests::ingest_pre_verified_events(events, ctx),
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::IngestPreVerifiedEventsForSubId { sub_id, events, ack } =>
            cmd_interests::ingest_pre_verified_events_for_sub_id(sub_id, events, ack, ctx),
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::TriggerGcStep { ack } =>
            cmd_interests::trigger_gc_step(ack, ctx),
    }
}
