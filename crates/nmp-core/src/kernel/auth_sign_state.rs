//! V-06 / #960 — NIP-42 AUTH signer-binding state + the remote-account async
//! AUTH queue, extracted from `kernel/mod.rs` to keep that file within its size
//! budget.
//!
//! The user-identity lanes (Content + Indexer) AUTH as the active account. A
//! *local* key binds a synchronous [`AuthSignerFn`] (resolved inline by the AUTH
//! handler); a *remote* (NIP-46 / NIP-55) account binds only its AUTH pubkey and
//! enqueues a [`PendingAuthSign`] for the async signer port. The two bindings are
//! kept disjoint — a role is in `auth_signers` XOR `auth_remote_pubkeys`, never
//! both. All the binding / clearing / draining methods that maintain that
//! invariant live here, alongside the [`PendingAuthSign`] type they enqueue.

use std::sync::Arc;

use super::{AuthSignerFn, Kernel, RelayAuthCredentials};
use crate::relay::RelayRole;

/// V-06 / #960 — a NIP-42 AUTH kind:22242 that a *remote* (NIP-46 / NIP-55)
/// account must sign through the async signer port.
///
/// When the active account is a remote signer, the kernel cannot sign the AUTH
/// challenge synchronously (only the broker holds the key). Instead
/// `handle_auth_challenge` builds the unsigned event and enqueues one of these;
/// the actor drains `take_pending_auth_signs()` after each inbound frame, routes
/// the `unsigned` through the same `sign_*_nonblocking` seam every other write
/// uses, and on resolution re-enters [`Kernel::dispatch_signed_auth`]. The relay
/// stays `ChallengeReceived` until the signed frame resolves.
#[derive(Clone, Debug)]
pub struct PendingAuthSign {
    /// The relay lane the challenge arrived on (its bound credentials select the
    /// signing account).
    pub role: RelayRole,
    /// The delivering relay URL — stamped on the kind:22242 `relay` tag and used
    /// to route the signed AUTH frame back to the same socket (NIP-42 replay
    /// binding).
    pub relay_url: String,
    /// The unsigned kind:22242 the remote signer must sign as-is.
    pub unsigned: crate::substrate::UnsignedEvent,
    /// The verbatim challenge — re-validated against the signed event on
    /// re-entry (`validate_signed_for`).
    pub challenge: String,
}

impl Kernel {
    /// Compat wrapper: bind the same identity signer to every user-identity
    /// relay role (Content + Indexer). Replaces any previously-bound identity
    /// signer on those roles; other roles (e.g. NWC `Wallet`) are unaffected.
    /// FFI bridge that surfaces this from Swift is T59
    /// (filed in `docs/perf/pending-user-decisions.md`).
    pub(crate) fn bind_auth_signer(&mut self, pubkey_hex: String, signer: AuthSignerFn) {
        self.auth_signers.insert(
            RelayRole::Content,
            RelayAuthCredentials {
                signer: Arc::clone(&signer),
                pubkey_hex: pubkey_hex.clone(),
            },
        );
        self.auth_signers.insert(
            RelayRole::Indexer,
            RelayAuthCredentials { signer, pubkey_hex },
        );
        // Keep the local-vs-remote AUTH binding disjoint: a synchronous local
        // signer supersedes any prior remote-pubkey binding on these lanes.
        self.auth_remote_pubkeys.remove(&RelayRole::Content);
        self.auth_remote_pubkeys.remove(&RelayRole::Indexer);
    }

    /// V-06 / #960 — bind the identity AUTH *pubkey* for a remote (NIP-46 /
    /// NIP-55) account on the user-identity lanes (Content + Indexer). The
    /// kernel knows whom to AUTH as but holds no synchronous signer, so a
    /// challenge on these lanes enqueues a [`PendingAuthSign`] for the async
    /// signer port rather than signing inline. Supersedes any prior synchronous
    /// signer on these lanes (disjoint with `bind_auth_signer`).
    pub(crate) fn bind_auth_remote(&mut self, pubkey_hex: String) {
        self.auth_signers.remove(&RelayRole::Content);
        self.auth_signers.remove(&RelayRole::Indexer);
        self.auth_remote_pubkeys
            .insert(RelayRole::Content, pubkey_hex.clone());
        self.auth_remote_pubkeys
            .insert(RelayRole::Indexer, pubkey_hex);
    }

    /// Compat wrapper: drop the identity signer for the user-identity roles
    /// (Content + Indexer). Other roles (e.g. NWC `Wallet`) are unaffected —
    /// use `clear_relay_auth_signer(role)` for per-role clearing.
    pub(crate) fn clear_auth_signer(&mut self) {
        self.auth_signers.remove(&RelayRole::Content);
        self.auth_signers.remove(&RelayRole::Indexer);
        // V-06: clear the remote AUTH-pubkey binding too — "no active account /
        // signed out" means neither a local signer nor a remote pubkey can AUTH.
        self.auth_remote_pubkeys.remove(&RelayRole::Content);
        self.auth_remote_pubkeys.remove(&RelayRole::Indexer);
    }

    /// V-06 / #960 — drain the AUTH kind:22242 events awaiting a remote
    /// signature. The actor calls this after each inbound frame and routes each
    /// through the async signer port (D8: one non-blocking move, never a wait).
    pub(crate) fn take_pending_auth_signs(&mut self) -> Vec<PendingAuthSign> {
        std::mem::take(&mut self.pending_auth_signs)
    }

    /// Test-only: the per-role AUTH driver FSM state (`None` if the role has no
    /// driver yet). Pins the NIP-42 async-AUTH state transitions in
    /// `actor/nip42_async_auth_tests.rs`.
    #[cfg(test)]
    pub(crate) fn relay_auth_state_for_test(
        &self,
        role: RelayRole,
    ) -> Option<crate::subs::RelayAuthState> {
        self.auth_drivers.get(&role).map(|d| d.state.clone())
    }
}
