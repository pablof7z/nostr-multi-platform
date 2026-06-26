//! Bounded inbox drain loop for `BrowserRuntime`.
//!
//! `drain_inbox` is the single entry point called by `BrowserRuntime::pump()`.
//! It drains up to [`BROWSER_COMMAND_DRAIN_BUDGET`] `ActorCommand`s from the
//! inbox channel without blocking, applies each to the kernel via
//! `KernelReducer::apply_actor_command`, and accumulates the resulting outbound
//! frames + host events.
//!
//! # D4 (single-writer)
//!
//! `drain_inbox` takes `&mut KernelReducer` — exactly one call site (the
//! owning `BrowserRuntime`) may hold this borrow. The wasm32 runtime is
//! inherently single-threaded; on native tests the `&mut` borrow enforces
//! exclusion.
//!
//! # D8 (no blocking, bounded work)
//!
//! `mpsc::Receiver::try_recv` is used — non-blocking. The loop stops at
//! [`BROWSER_COMMAND_DRAIN_BUDGET`] applied commands per pump (mirroring the
//! native actor's `COMMAND_DRAIN_BUDGET` fairness budget in
//! `nmp-core/src/actor/inbox.rs`) so a flood cannot monopolise one turn.
//! [`DrainOutcome::yielded`] is set when the budget was hit, telling the host to
//! pump again; any leftover mail stays queued in the channel (never dropped).

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};

use nmp_core::actor::ActorMail;
use nmp_core::{CommandApplyOutcome, KernelReducer, OutboundMessage};

use super::event::BrowserRuntimeEvent;
use super::PendingSignedPublish;
use crate::signer::{broker_sign_request, CapabilityProviderRegistry, SignerCompletionTx};

/// Maximum number of commands applied per `pump()` turn.
///
/// Mirrors `nmp-core`'s native `COMMAND_DRAIN_BUDGET` (= 64). Bounding the
/// per-turn work keeps one pump from starving the host's event loop under a
/// command flood; leftover mail stays in the channel and is reported via
/// [`DrainOutcome::yielded`] so the host re-pumps.
pub(super) const BROWSER_COMMAND_DRAIN_BUDGET: usize = 64;

/// Result of one bounded drain pass.
pub(super) struct DrainOutcome {
    /// Outbound relay frames produced by applied commands this turn.
    pub(super) outbound: Vec<OutboundMessage>,
    /// Host events (sign requests, command failures) produced this turn.
    pub(super) events: Vec<BrowserRuntimeEvent>,
    /// True when the drain budget was exhausted this turn — the host should
    /// call `pump()` again to make progress on any remaining queued mail.
    pub(super) yielded: bool,
}

/// Drain up to [`BROWSER_COMMAND_DRAIN_BUDGET`] commands and apply each.
///
/// Each command's [`CommandApplyOutcome`] is honored:
/// * `Applied` → its outbound frames are collected.
/// * `NeedsSign` → the publish continuation is parked in `pending` keyed on the
///   sign correlation id. `broker_sign_request` is called; if a provider is
///   found it dispatches the sign (LocalKey: inline; NIP-07/wasm: async via
///   channel). Only when no provider is found is
///   [`BrowserRuntimeEvent::SignRequest`] emitted for host-brokering (D6 —
///   never a silent drop).
/// * `Unsupported` → a [`BrowserRuntimeEvent::CommandFailed`] is emitted.
pub(super) fn drain_inbox(
    reducer: &mut KernelReducer,
    rx: &Receiver<ActorMail>,
    pending: &mut HashMap<String, PendingSignedPublish>,
    registry: &CapabilityProviderRegistry,
    completion_tx: &SignerCompletionTx,
) -> DrainOutcome {
    let mut outbound: Vec<OutboundMessage> = Vec::new();
    let mut events: Vec<BrowserRuntimeEvent> = Vec::new();
    let mut applied = 0usize;

    while applied < BROWSER_COMMAND_DRAIN_BUDGET {
        let cmd = match rx.try_recv() {
            Ok(ActorMail::Command(cmd)) => cmd,
            // `ActorMail::Relay` exists only when `nmp-core/native` is unified
            // into this build (workspace feature unification adds the cfg-gated
            // variant). The browser ignores relay mail here — it has its own
            // relay driver (#2050). Unreachable under `--no-default-features`
            // (Command is then the only variant), so the wildcard is allow'd.
            #[allow(unreachable_patterns)]
            Ok(_) => continue,
            // Channel drained or sender gone: no more work this turn and nothing
            // was left behind, so the host need not re-pump on our account.
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {
                return DrainOutcome {
                    outbound,
                    events,
                    yielded: false,
                };
            }
        };

        applied += 1;
        match reducer.apply_actor_command(cmd) {
            CommandApplyOutcome::Applied(msgs) => {
                outbound.extend(msgs);
            }
            CommandApplyOutcome::NeedsSign {
                request,
                target,
                action_correlation_id,
            } => {
                // Park the publish continuation under the sign correlation id.
                pending.insert(
                    request.correlation_id.clone(),
                    PendingSignedPublish {
                        action_correlation_id,
                        target,
                    },
                );
                // Try to auto-broker using a registered provider (LocalKey:
                // inline; NIP-07/wasm: async via spawn_local → channel).
                // If no provider is found, emit SignRequest for host-brokering
                // (never silently drop — D6).
                let brokered = broker_sign_request(
                    registry,
                    &request.correlation_id,
                    &request.account_pubkey,
                    &request.unsigned_json,
                    completion_tx,
                );
                if !brokered {
                    events.push(BrowserRuntimeEvent::SignRequest {
                        correlation_id: request.correlation_id,
                        account_pubkey: request.account_pubkey,
                        unsigned_json: request.unsigned_json,
                    });
                }
            }
            CommandApplyOutcome::Unsupported { reason } => {
                events.push(BrowserRuntimeEvent::CommandFailed { reason });
            }
        }
    }

    // Budget exhausted. Any further mail stays queued (never consumed/dropped);
    // signal the host to re-pump. A spurious yield (channel actually empty) costs
    // only one cheap `try_recv` Empty on the next pump.
    DrainOutcome {
        outbound,
        events,
        yielded: true,
    }
}
