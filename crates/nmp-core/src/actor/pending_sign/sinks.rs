//! The unified parked-op type and its terminal sinks (ADR-0050 §D2).
//!
//! Before this module the actor parked remote signer ops in **two** separate
//! `Vec`s with duplicated poll/timeout machinery:
//!
//! * `PendingSign` (publish) — drained by a ~90-line inline block in the actor
//!   loop that owned outbox routing, toasts, and `record_action_failure`.
//! * `PendingSignReturn` (sign-and-return / generic sign port) — drained by
//!   `resolve_pending_sign_return`.
//!
//! This module collapses both into ONE [`ParkedOp`] carried in ONE `Vec`,
//! drained once per idle tick by [`super::drain::resolve_parked_op`]. The
//! terminal behaviour is selected by [`ParkedOpSink`]:
//!
//! * [`ParkedOpSink::SignedEventsProjection`] — write signed JSON / error into
//!   `signed_events[correlation_id]` (the `SignEventForReturn` seam).
//! * [`ParkedOpSink::SignContinuation`] — invoke the boxed sign continuation
//!   (the generic `SignEventForAccount` port).
//! * [`ParkedOpSink::CipherContinuation`] — invoke the boxed cipher
//!   continuation with the resolved plaintext/ciphertext (the new
//!   `Nip44EncryptForAccount` / `Nip44DecryptForAccount` port, §D1). This is the
//!   ONLY sink that parks a `SignerOp<String>`; the other three park a
//!   `SignerOp<SignedEvent>`.
//! * [`ParkedOpSink::Publish`] — route the signed event into the publish engine.
//!   Because the actor loop owns relay routing, this sink does **not** call the
//!   engine itself: the drain *returns* a [`PublishObligation`] to the loop,
//!   which calls `publish_signed_to_with_correlation` and routes the frames.
//!
//! D8: the op is polled with a non-blocking `try_recv` (`SignerOp::poll`); the
//! `deadline` is the wall-clock timeout gate, never a sleep. D13: only a
//! `SignedEvent` / ciphertext / plaintext ever crosses a continuation, never raw
//! key bytes.

use crate::publish::PublishTarget;
use crate::substrate::SignedEvent;
use nmp_signer_iface::SignerOp;
use std::time::Instant;

use crate::actor::{CipherContinuation, SignContinuation};

/// Where a resolved [`ParkedOp`] delivers its outcome, and the in-flight op
/// itself.
///
/// The op lives *inside* the sink (not beside it) because the cipher port parks
/// a `SignerOp<String>` while every other sink parks a `SignerOp<SignedEvent>` —
/// one struct field could not type both. The boxed continuations live in an
/// `Option` so the drain — which borrows each parked op `&mut` via `retain_mut`
/// — can `.take()` the `FnOnce` before calling it.
pub(crate) enum ParkedOpSink {
    /// Write the resolved signed JSON / error into
    /// `signed_events[correlation_id]` (the `SignEventForReturn` seam).
    SignedEventsProjection {
        op: SignerOp<SignedEvent>,
        correlation_id: String,
    },
    /// Invoke the boxed sign continuation with the resolved sign outcome (the
    /// generic `SignEventForAccount` backend-transparent port).
    SignContinuation {
        op: SignerOp<SignedEvent>,
        continuation: Option<SignContinuation>,
    },
    /// Invoke the boxed cipher continuation with the resolved ciphertext /
    /// plaintext (the `Nip44EncryptForAccount` / `Nip44DecryptForAccount`
    /// port, §D1). The sole `SignerOp<String>` sink.
    CipherContinuation {
        op: SignerOp<String>,
        continuation: Option<CipherContinuation>,
    },
    /// V-06 / #960 — route the resolved signed kind:22242 back into the kernel's
    /// NIP-42 AUTH handler. Like [`Publish`](ParkedOpSink::Publish) this sink
    /// does NOT touch the kernel from the drain: it returns an [`AuthObligation`]
    /// to the actor loop, which calls `Kernel::dispatch_signed_auth` /
    /// `fail_auth_sign` and routes the outbound AUTH frame. This is the remote
    /// (NIP-46 / NIP-55) half of the ONE async sign seam — a local key never
    /// parks here (it resolves inline in `handle_auth_challenge`).
    Auth {
        op: SignerOp<SignedEvent>,
        /// The relay lane the challenge arrived on.
        role: crate::relay::RelayRole,
        /// The delivering relay URL (NIP-42 replay binding + routing target).
        relay_url: String,
        /// The verbatim challenge — re-validated against the signed event.
        challenge: String,
    },
    /// Route the resolved signed event into the publish engine. The drain
    /// returns the routing obligation to the actor loop (which owns relay
    /// routing) rather than calling the engine itself.
    Publish {
        op: SignerOp<SignedEvent>,
        /// `p_tags` forwarded to `publish_signed_to_with_correlation`. Empty for
        /// every current publish callsite (the engine resolves NIP-65 outbox
        /// relays itself); carried so a p-tagged publish can route without
        /// another signature change.
        p_tags: Vec<String>,
        /// D3 routing mode: `Auto` (NIP-65 outbox) for kind:1/3/7, `Explicit`
        /// for host-pinned action executors (e.g. NIP-29 group events) so the
        /// relay pin survives the remote-sign round-trip.
        target: PublishTarget,
        /// Dispatched-action `correlation_id` to settle under once the parked
        /// publish lands, when it differs from the eventual event id. `None`
        /// for `react` / `follow` / non-dispatched publishes.
        correlation_id_override: Option<String>,
    },
}

/// A remote signer operation parked on the actor loop, awaiting the broker's
/// turn-around (NIP-46 kind:24133 / NIP-55 Intent reply). One `Vec<ParkedOp>`
/// holds every parked op regardless of terminal; the drain dispatches on
/// [`ParkedOp::sink`].
pub(crate) struct ParkedOp {
    /// Terminal sink + the in-flight op.
    pub sink: ParkedOpSink,
    /// Drop-dead time, sourced from the SIGNING account's
    /// `RemoteSignerHandle::op_timeout()` budget (ADR-0050 D4 — NIP-46 = 5s,
    /// NIP-55 = 90s) via `IdentityRuntime::sign_deadline_for` /
    /// `active_sign_deadline`. Past this the op is abandoned with an error
    /// terminal so the host never hangs (D6).
    pub deadline: Instant,
}

impl ParkedOp {
    /// Park a sign-and-return op resolving into `signed_events[correlation_id]`.
    #[must_use]
    pub fn signed_events_projection(
        op: SignerOp<SignedEvent>,
        correlation_id: String,
        deadline: Instant,
    ) -> Self {
        Self {
            sink: ParkedOpSink::SignedEventsProjection { op, correlation_id },
            deadline,
        }
    }

    /// Park a generic-sign-port op resolving into a boxed sign continuation.
    #[must_use]
    pub fn sign_continuation(
        op: SignerOp<SignedEvent>,
        continuation: SignContinuation,
        deadline: Instant,
    ) -> Self {
        Self {
            sink: ParkedOpSink::SignContinuation {
                op,
                continuation: Some(continuation),
            },
            deadline,
        }
    }

    /// Park a cipher-port op resolving into a boxed cipher continuation (§D1).
    #[must_use]
    pub fn cipher_continuation(
        op: SignerOp<String>,
        continuation: CipherContinuation,
        deadline: Instant,
    ) -> Self {
        Self {
            sink: ParkedOpSink::CipherContinuation {
                op,
                continuation: Some(continuation),
            },
            deadline,
        }
    }

    /// Park a publish op resolving into the publish engine via the loop.
    #[must_use]
    pub fn publish(
        op: SignerOp<SignedEvent>,
        p_tags: Vec<String>,
        target: PublishTarget,
        correlation_id_override: Option<String>,
        deadline: Instant,
    ) -> Self {
        Self {
            sink: ParkedOpSink::Publish {
                op,
                p_tags,
                target,
                correlation_id_override,
            },
            deadline,
        }
    }

    /// V-06 / #960 — park a NIP-42 AUTH sign resolving into the kernel's AUTH
    /// handler via an [`AuthObligation`] handed back to the loop.
    #[must_use]
    pub fn auth(
        op: SignerOp<SignedEvent>,
        role: crate::relay::RelayRole,
        relay_url: String,
        challenge: String,
        deadline: Instant,
    ) -> Self {
        Self {
            sink: ParkedOpSink::Auth {
                op,
                role,
                relay_url,
                challenge,
            },
            deadline,
        }
    }

    /// True once the op has overrun its deadline.
    pub fn timed_out(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// V-06 / #960 — the NIP-42 AUTH-routing obligation a [`ParkedOpSink::Auth`]
/// hands back to the actor loop when its op resolves. The loop owns relay
/// routing + the `&mut Kernel` re-entry, so the drain returns this instead of
/// calling the kernel itself.
pub(crate) enum AuthObligation {
    /// The signed kind:22242 is ready — call `Kernel::dispatch_signed_auth` with
    /// these fields and route the resulting AUTH frame.
    Dispatch {
        role: crate::relay::RelayRole,
        relay_url: String,
        challenge: String,
        signed: SignedEvent,
    },
    /// The sign failed / timed out — call `Kernel::fail_auth_sign` so the relay
    /// drives to `Failed` and any deferred REQs fail closed (T76 / D6).
    Failed {
        role: crate::relay::RelayRole,
        relay_url: String,
        reason: String,
    },
}

/// The publish-routing obligation a [`ParkedOpSink::Publish`] hands back to the
/// actor loop when its op resolves. The loop owns relay routing, so the drain
/// returns this instead of calling the engine itself.
pub(crate) enum PublishObligation {
    /// The signed event is ready — call `publish_signed_to_with_correlation`
    /// with these fields and route the resulting frames.
    Publish {
        signed: SignedEvent,
        p_tags: Vec<String>,
        target: PublishTarget,
        correlation_id_override: Option<String>,
    },
    /// The sign failed / timed out — surface `toast`, and if
    /// `correlation_id_override` is `Some`, record a terminal `"failed"` verdict
    /// so the host spinner clears (D6).
    Failed {
        toast: String,
        correlation_id_override: Option<String>,
    },
}
