//! The single parked-op drain (ADR-0050 §D2).
//!
//! One `retain_mut` pass over one `Vec<ParkedOp>` replaces the two former
//! drains (the inline publish block in `actor/mod.rs` and
//! `resolve_pending_sign_return`). [`resolve_parked_op`] polls one op, and on a
//! resolved / errored / timed-out outcome dispatches it to the op's terminal:
//!
//! * projection / continuation sinks resolve directly against the kernel /
//!   boxed continuation here;
//! * the [`Publish`](super::sinks::ParkedOpSink::Publish) sink returns a
//!   [`PublishObligation`] to the actor loop, which owns relay routing, so the
//!   engine call + frame routing + `emit_now` stay in the loop exactly as
//!   before.
//!
//! D8: `SignerOp::poll` is a non-blocking `try_recv`; the deadline is the
//! wall-clock timeout gate. D6: every terminal (signed / broker error / timeout)
//! reaches its sink — a dropped continuation would hang the host spinner.

use nmp_signer_iface::SignerOp;

use super::sinks::{AuthObligation, ParkedOp, ParkedOpSink, PublishObligation};

/// What the actor loop must do after [`resolve_parked_op`] handled one op.
pub(crate) struct DrainOutcome {
    /// `true` to KEEP the op (still pending, deadline not elapsed); `false` once
    /// it has resolved / errored / timed out so `retain_mut` drops it.
    pub keep: bool,
    /// A publish-routing obligation the loop must execute (the `Publish` sink
    /// only). `None` for every other sink and for a still-pending op.
    pub publish: Option<PublishObligation>,
    /// V-06 / #960 — a NIP-42 AUTH-routing obligation the loop must execute (the
    /// `Auth` sink only). `None` for every other sink and for a still-pending op.
    pub auth: Option<AuthObligation>,
    /// `true` when this call changed kernel state (a terminal was recorded), so
    /// the loop emits a snapshot this tick rather than waiting for `flush_due`.
    pub changed: bool,
}

impl DrainOutcome {
    fn keep() -> Self {
        Self {
            keep: true,
            publish: None,
            auth: None,
            changed: false,
        }
    }

    /// A resolved (drop-the-op) outcome that changed kernel state, carrying no
    /// loop obligation — the continuation / projection sinks settle in-drain.
    fn resolved() -> Self {
        Self {
            keep: false,
            publish: None,
            auth: None,
            changed: true,
        }
    }
}

/// A resolved parked-op outcome, distinguishing the broker-error and
/// deadline-timeout terminals (the publish sink words its toast differently for
/// each, preserving the prior inline drain). `Pending` means keep the op.
enum Resolved<T> {
    Pending,
    Ok(T),
    /// Broker rejected the op / its channel dropped, carrying the error string.
    BrokerErr(String),
    /// The op overran its `op_timeout()` deadline before the broker responded.
    TimedOut,
}

impl<T> Resolved<T> {
    /// Collapse to the `Result<T, String>` the projection / continuation sinks
    /// deliver (they do not distinguish timeout from broker error). Returns
    /// `None` while still pending.
    fn into_result(self, timeout_msg: &str) -> Option<Result<T, String>> {
        match self {
            Resolved::Pending => None,
            Resolved::Ok(v) => Some(Ok(v)),
            Resolved::BrokerErr(e) => Some(Err(e)),
            Resolved::TimedOut => Some(Err(timeout_msg.to_string())),
        }
    }
}

/// Poll one op, mapping pending/ready/error and folding the deadline check in.
/// Polls FIRST so a result that landed on the same tick as the deadline is not
/// lost to the timeout branch (preserved from the prior drains).
fn poll<T: Send + 'static>(op: &mut SignerOp<T>, timed_out: bool) -> Resolved<T> {
    match op.poll() {
        None => {
            if timed_out {
                Resolved::TimedOut
            } else {
                Resolved::Pending
            }
        }
        Some(Ok(v)) => Resolved::Ok(v),
        Some(Err(e)) => Resolved::BrokerErr(e.to_string()),
    }
}

/// Drain one parked op against its sink. Called once per idle tick from the
/// actor loop's single `retain_mut` over `parked_ops` (D8 — one non-blocking
/// `poll()`, never a wait).
pub(crate) fn resolve_parked_op(
    parked: &mut ParkedOp,
    kernel: &mut crate::kernel::Kernel,
) -> DrainOutcome {
    let timed_out = parked.timed_out();
    match &mut parked.sink {
        ParkedOpSink::SignedEventsProjection { op, correlation_id } => {
            let Some(outcome) = poll(op, timed_out).into_result("signing timed out") else {
                return DrainOutcome::keep();
            };
            // D13 `SignEventForReturn`: write signed JSON / error into the
            // `signed_events[correlation_id]` projection.
            let recorded =
                outcome.map(|signed| crate::actor::dispatch::signed_event_to_json(&signed));
            kernel.record_signed_event_return(correlation_id, recorded);
            DrainOutcome::resolved()
        }
        ParkedOpSink::SignContinuation { op, continuation } => {
            let Some(outcome) = poll(op, timed_out).into_result("signing timed out") else {
                return DrainOutcome::keep();
            };
            // Generic `SignEventForAccount` port: take the boxed continuation
            // out of the `&mut` sink (an `FnOnce` cannot be called through
            // `&mut`) and invoke it with the resolved outcome. It runs on the
            // actor thread and only enqueues further work (D8); on `Err` it must
            // itself resolve the host's action terminal (D6).
            if let Some(continuation) = continuation.take() {
                continuation.call(outcome);
            }
            DrainOutcome::resolved()
        }
        ParkedOpSink::CipherContinuation { op, continuation } => {
            let Some(outcome) = poll(op, timed_out).into_result("nip44 operation timed out") else {
                return DrainOutcome::keep();
            };
            // §D1 cipher port: deliver the resolved ciphertext / plaintext (or
            // an error string) to the boxed cipher continuation. Same take-then-
            // call invariant and on-actor-thread / non-blocking contract as the
            // sign continuation; D13 — only ciphertext/plaintext crosses, never
            // key material.
            if let Some(continuation) = continuation.take() {
                continuation.call(outcome);
            }
            DrainOutcome::resolved()
        }
        ParkedOpSink::Auth {
            op,
            role,
            relay_url,
            challenge,
        } => {
            // V-06 / #960: the remote AUTH sign resolved. Hand an obligation back
            // to the loop, which owns the `&mut Kernel` re-entry + relay routing.
            // Pending → keep; a timeout / broker error fails the AUTH closed.
            let obligation = match poll(op, timed_out) {
                Resolved::Pending => return DrainOutcome::keep(),
                Resolved::Ok(signed) => AuthObligation::Dispatch {
                    role: *role,
                    relay_url: std::mem::take(relay_url),
                    challenge: std::mem::take(challenge),
                    signed,
                },
                Resolved::TimedOut => AuthObligation::Failed {
                    role: *role,
                    relay_url: std::mem::take(relay_url),
                    reason: "AUTH sign timed out".to_string(),
                },
                Resolved::BrokerErr(e) => AuthObligation::Failed {
                    role: *role,
                    relay_url: std::mem::take(relay_url),
                    reason: format!("AUTH sign failed: {e}"),
                },
            };
            DrainOutcome {
                keep: false,
                publish: None,
                auth: Some(obligation),
                changed: true,
            }
        }
        ParkedOpSink::Publish {
            op,
            p_tags,
            target,
            correlation_id_override,
        } => {
            let obligation = match poll(op, timed_out) {
                Resolved::Pending => return DrainOutcome::keep(),
                Resolved::Ok(signed) => PublishObligation::Publish {
                    signed,
                    p_tags: std::mem::take(p_tags),
                    target: target.clone(),
                    correlation_id_override: correlation_id_override.clone(),
                },
                // Preserve the prior inline drain's exact toast wording: a
                // timeout surfaced "remote sign timed out"; a broker error
                // surfaced "remote sign failed: {e}".
                Resolved::TimedOut => PublishObligation::Failed {
                    toast: "remote sign timed out".to_string(),
                    correlation_id_override: correlation_id_override.clone(),
                },
                Resolved::BrokerErr(e) => PublishObligation::Failed {
                    toast: format!("remote sign failed: {e}"),
                    correlation_id_override: correlation_id_override.clone(),
                },
            };
            DrainOutcome {
                keep: false,
                publish: Some(obligation),
                auth: None,
                changed: true,
            }
        }
    }
}
