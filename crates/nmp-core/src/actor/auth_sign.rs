//! V-06 / #960 — NIP-42 async-AUTH drain + obligation execution for the actor
//! loop, extracted from `actor/mod.rs` to keep that file within its size budget.
//!
//! When the active account on an AUTH-required relay lane is a *remote* signer
//! (NIP-46 / NIP-55) the kernel cannot sign the kind:22242 challenge inline — it
//! enqueues a [`PendingAuthSign`] instead. Two phases live here, both called from
//! the actor main loop:
//!
//! 1. [`drain_pending_auth_signs`] — drains `kernel.take_pending_auth_signs()`
//!    after each inbound frame and routes every request through the SAME async
//!    signer port every other write uses (`sign_with_account_nonblocking`),
//!    parking the pending ops under the [`ParkedOpSink::Auth`] sink.
//! 2. [`run_auth_obligations`] — executes the [`AuthObligation`]s the `Auth`
//!    sink hands back once parked ops resolve, re-entering
//!    `Kernel::dispatch_signed_auth` (or `fail_auth_sign`) and routing outbound.
//!
//! Both phases own the same relay-routing surface, bundled here as [`RouteCtx`]
//! so the two call sites in `run_actor_with_observers` stay one-liners and the
//! routing fields are threaded once rather than per call.

use std::collections::HashMap;

use nmp_network::pool::Pool;

use super::commands::{self, IdentityRuntime};
use super::pending_sign::{AuthObligation, ParkedOp};
use super::relay_mgmt::route_dispatch_outbound;
use super::RelayControl;
use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, OutboundMessage};

/// The mutable relay-routing surface the actor loop owns. Bundled so the AUTH
/// drain / obligation phases borrow it once instead of taking six parameters
/// each. Field semantics are identical to the corresponding locals in
/// `run_actor_with_observers`; this only groups the borrows.
pub(super) struct RouteCtx<'a> {
    pub running: bool,
    pub queued_publish_outbound: &'a mut Vec<OutboundMessage>,
    pub relay_controls: &'a mut HashMap<CanonicalRelayUrl, RelayControl>,
    pub slot_to_url: &'a mut HashMap<u32, CanonicalRelayUrl>,
    pub pool: &'a Pool,
    pub next_relay_generation: &'a mut u64,
}

impl RouteCtx<'_> {
    /// Route a freshly produced AUTH frame batch through the relay pool, exactly
    /// as the inline call site did.
    fn route(&mut self, kernel: &mut Kernel, outbound: Vec<OutboundMessage>) {
        route_dispatch_outbound(
            self.running,
            self.queued_publish_outbound,
            self.relay_controls,
            self.slot_to_url,
            self.pool,
            kernel,
            self.next_relay_generation,
            outbound,
        );
    }
}

/// V-06 / #960 — drain the kernel-emitted NIP-42 AUTH signs.
///
/// `handle_message` enqueues an AUTH kind:22242 for any relay lane whose active
/// account is a REMOTE signer (no synchronous `AuthSignerFn`). Route each through
/// the SAME async signer port every other write uses
/// (`sign_with_account_nonblocking` → park under the `Auth` sink). A local key
/// never reaches here — it resolves inline in the kernel. The actor loop is the
/// single place that owns both `identity` and `parked_ops`, so the drain lives
/// here rather than threading two params through `handle_relay_event`.
pub(super) fn drain_pending_auth_signs(
    kernel: &mut Kernel,
    identity: &IdentityRuntime,
    parked_ops: &mut Vec<ParkedOp>,
    route: &mut RouteCtx<'_>,
) {
    for req in kernel.take_pending_auth_signs() {
        let signer_pk = req.unsigned.pubkey.clone();
        match commands::sign_with_account_nonblocking(identity, &signer_pk, &req.unsigned) {
            Err(reason) => kernel.fail_auth_sign(req.role, &req.relay_url, reason),
            Ok(mut op) => match op.poll() {
                // Defensive: a signer that resolves Ready inline (e.g. a key
                // promoted to local mid-session) dispatches immediately.
                Some(Ok(signed)) => {
                    let outbound = kernel.dispatch_signed_auth(
                        req.role,
                        &req.relay_url,
                        &req.challenge,
                        signed,
                    );
                    route.route(kernel, outbound);
                }
                Some(Err(e)) => kernel.fail_auth_sign(req.role, &req.relay_url, e.to_string()),
                None => {
                    let deadline = identity.sign_deadline_for(Some(&signer_pk));
                    parked_ops.push(ParkedOp::auth(
                        op,
                        req.role,
                        req.relay_url,
                        req.challenge,
                        deadline,
                    ));
                }
            },
        }
    }
}

/// V-06 / #960 — execute the NIP-42 AUTH obligations the `Auth` sink handed back.
///
/// A resolved remote sign re-enters `dispatch_signed_auth` (validate →
/// Authenticating → emit the CLIENT-AUTH frame, routed back to the delivering
/// socket); a failure / timeout drives the relay to `Failed` and fails closed
/// (T76). The loop owns the `&mut Kernel` re-entry + relay routing, so this runs
/// after the parked-op retain (the drain's `&mut kernel` borrow has ended).
pub(super) fn run_auth_obligations(
    kernel: &mut Kernel,
    auth_obligations: Vec<AuthObligation>,
    route: &mut RouteCtx<'_>,
) {
    for obligation in auth_obligations {
        match obligation {
            AuthObligation::Dispatch {
                role,
                relay_url,
                challenge,
                signed,
            } => {
                let outbound = kernel.dispatch_signed_auth(role, &relay_url, &challenge, signed);
                route.route(kernel, outbound);
            }
            AuthObligation::Failed {
                role,
                relay_url,
                reason,
            } => {
                kernel.fail_auth_sign(role, &relay_url, reason);
            }
        }
    }
}
