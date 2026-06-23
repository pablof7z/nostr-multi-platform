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
//! | `mod.rs` | `ActorContext`, `build_open_interest`, `dispatch_command` (family-level delegator) |
//! | `cmd_lifecycle.rs` | `Lifecycle(LifecycleCommand)` arm |
//! | `cmd_identity.rs` | `Identity(IdentityCommand)` arm |
//! | `cmd_publish.rs` | `Publish` / `Contacts` / `Relay` / `ActionLedger` arms |
//! | `cmd_interests.rs` | `Interests(InterestsCommand)` + `TestSupport` arms |
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
use super::{
    ActionLedgerCommand, ActorCommand, ActorConfig, ContactsCommand, IdentityCommand,
    InterestsCommand, LifecycleCommand, PublishCommand, RelayCommand, RefsCommand, RelayControl,
    SignCommand,
};
#[cfg(any(test, feature = "test-support"))]
use super::TestSupportCommand;
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
/// immediately after dispatch, so every other call site in the actor loop keeps
/// using the original locals untouched. The lifetime `'a` ties the struct to
/// those stack-resident locals — no heap allocation, no ownership transfer,
/// the actor loop still owns every field.
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

/// Family-level dispatch delegator (ADR-0065). Matches the `ActorCommand`
/// family first, then delegates to the per-family `dispatch` function in the
/// matching `cmd_*.rs` sub-module.
pub(super) fn dispatch_command(
    command: ActorCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match command {
        ActorCommand::Lifecycle(cmd) => cmd_lifecycle::dispatch(cmd, ctx),
        ActorCommand::Identity(cmd) => cmd_identity::dispatch(cmd, ctx),
        ActorCommand::Sign(cmd) => dispatch_sign(cmd, ctx),
        ActorCommand::Publish(cmd) => cmd_publish::dispatch_publish(cmd, ctx),
        ActorCommand::Contacts(cmd) => cmd_publish::dispatch_contacts(cmd, ctx),
        ActorCommand::Relay(cmd) => cmd_publish::dispatch_relay(cmd, ctx),
        ActorCommand::Refs(cmd) => dispatch_refs(cmd, ctx),
        ActorCommand::Interests(cmd) => cmd_interests::dispatch(cmd, ctx),
        ActorCommand::ActionLedger(cmd) => cmd_publish::dispatch_action_ledger(cmd, ctx),
        ActorCommand::Protocol(cmd) => cmd_protocol::protocol(cmd, ctx),
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
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::TestSupport(cmd) => dispatch_test_support(cmd, ctx),
    }
}

/// ADR-0050 signer-session capability port dispatch (the `Sign` family).
/// Routes through `signer_port_dispatch` — local keys resolve inline, remote
/// signers park under the continuation sink.
fn dispatch_sign(cmd: SignCommand, ctx: &mut ActorContext<'_>) -> Option<Vec<OutboundMessage>> {
    match cmd {
        SignCommand::EventForReturn {
            account_pubkey,
            unsigned_json,
            correlation_id,
        } => cmd_identity::sign_event_for_return(account_pubkey, unsigned_json, correlation_id, ctx),
        SignCommand::EventForAccount { unsigned, signer_pubkey, continuation } =>
            signer_port_dispatch::sign_for_account(ctx, &unsigned, signer_pubkey, continuation),
        SignCommand::Nip44EncryptForAccount { peer_pubkey, plaintext, signer_pubkey, continuation } =>
            signer_port_dispatch::nip44_encrypt_for_account(
                ctx, &peer_pubkey, &plaintext, signer_pubkey, continuation,
            ),
        SignCommand::Nip44DecryptForAccount { peer_pubkey, ciphertext, signer_pubkey, continuation } =>
            signer_port_dispatch::nip44_decrypt_for_account(
                ctx, &peer_pubkey, &ciphertext, signer_pubkey, continuation,
            ),
    }
}

/// `Refs` family dispatch — thin delegators to the kernel's
/// `claim_event` / `release_event` / `resolve_ref` / `release_ref` one-liners.
fn dispatch_refs(cmd: RefsCommand, ctx: &mut ActorContext<'_>) -> Option<Vec<OutboundMessage>> {
    match cmd {
        RefsCommand::ClaimEvent { uri, consumer_id, force } => {
            let outbound = ctx.kernel.claim_event(uri, consumer_id, ctx.relays_ready, force);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        RefsCommand::ReleaseEvent { uri, consumer_id } => {
            let outbound = ctx.kernel.release_event(&uri, &consumer_id);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        RefsCommand::Resolve { namespace, key, consumer_id, shape, liveness, force, hints } => {
            let outbound = ctx.kernel.resolve_ref(
                namespace, key, consumer_id, shape, liveness, force, hints,
            );
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
        RefsCommand::Release { namespace, key, consumer_id } => {
            let outbound = ctx.kernel.release_ref(namespace, &key, &consumer_id);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(outbound)
        }
    }
}

/// `TestSupport` family dispatch (cfg-gated). Routes ingest/GC to
/// `cmd_interests` and the barrier ack inline.
#[cfg(any(test, feature = "test-support"))]
fn dispatch_test_support(
    cmd: TestSupportCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        TestSupportCommand::IngestPreVerifiedEvents(events) =>
            cmd_interests::ingest_pre_verified_events(events, ctx),
        TestSupportCommand::IngestPreVerifiedEventsForSubId { sub_id, events, ack } =>
            cmd_interests::ingest_pre_verified_events_for_sub_id(sub_id, events, ack, ctx),
        TestSupportCommand::TriggerGcStep { ack } => cmd_interests::trigger_gc_step(ack, ctx),
        TestSupportCommand::Barrier { ack } => {
            let _ = ack.send(());
            Some(Vec::new())
        }
    }
}

// Silence unused-import warnings for family types whose only use is the
// re-export chain (`pub use` in `actor_command.rs`). The dispatch *handlers*
// in `cmd_*.rs` name these types directly; this top-level module only matches
// on them. `#[allow(unused_imports)]` keeps the import list self-documenting.
#[allow(unused_imports)]
use super::commands::handle_lifecycle_event as _handle_lifecycle_event_marker;