//! Parked remote-signer ops — one unified park + one drain (ADR-0050 §D2).
//!
//! Background: every publish/sign path on the actor signs through a
//! `*_nonblocking` helper (`commands/identity.rs`) that hands back a raw
//! `SignerOp` without blocking the actor thread (D8). A local signer's op is
//! `Ready` and resolves inline; a remote signer's op is `Pending` and is parked
//! here in a single [`ParkedOp`] queue. The actor's idle section
//! `retain_mut`-drains every parked op once per tick via
//! [`drain::resolve_parked_op`] — a non-blocking `try_recv` — and runs the op's
//! terminal the moment the broker turns the request around.
//!
//! Before ADR-0050 §D2 this lived in TWO queues with duplicated machinery: a
//! publish queue (`PendingSign`) drained by a ~90-line inline block in
//! `actor/mod.rs`, and a sign-and-return queue (`PendingSignReturn`) drained by
//! `resolve_pending_sign_return`. Both collapsed into [`ParkedOp`] +
//! [`ParkedOpSink`]; the inline publish drain is deleted and its terminal
//! behaviour (outbox routing, toast, `record_action_failure`, immediate emit)
//! is preserved by the loop's [`PublishObligation`] handler.
//!
//! `deadline` bounds the wait: a broker that never responds within the signing
//! account's `RemoteSignerHandle::op_timeout()` budget (ADR-0050 D4 — NIP-46 =
//! 5s, NIP-55 = 90s) has its op dropped and an error terminal delivered to its
//! sink (D6 — the error becomes kernel state / a resolved continuation, the
//! actor never wedges).

mod drain;
mod sinks;

// The default parked-op deadline (`nmp_signer_iface::PENDING_SIGN_TIMEOUT`, 5s)
// lives in the leaf iface crate so it is available both here (native-gated park
// sites) and in the always-compiled `RemoteSignerHandle::op_timeout` default in
// `remote_signer.rs`. Individual signers override it via `op_timeout()` (e.g.
// `Nip55Signer` = 90s). The kernel never hard-codes NIP-55 — it reads the
// duration from the handle it already holds. Consumers import it directly from
// `nmp_signer_iface`; no re-export here.

pub(crate) use drain::resolve_parked_op;
pub(crate) use sinks::{AuthObligation, ParkedOp, PublishObligation};
// `ParkedOpSink` is named only by tests (the dispatch arms / drain construct it
// via `ParkedOp::*` constructors); gate the re-export so a non-test lib build
// does not warn it unused.
#[cfg(test)]
pub(crate) use sinks::ParkedOpSink;

#[cfg(test)]
mod tests {
    //! Unit tests for the unified parked-op path. These pin the *async*
    //! `SignerOp::Pending` behaviour the actor loop relies on across all four
    //! sinks — distinct from `remote_signer_tests.rs`, whose `StubSigner`
    //! always returns a ready-now op so the park queue never accumulates.
    use super::sinks::{ParkedOp, ParkedOpSink};
    use crate::actor::{CipherContinuation, SignContinuation};
    use crate::publish::PublishTarget;
    use crate::substrate::{SignedEvent, UnsignedEvent};
    use nmp_signer_iface::{SignerError, SignerOp, PENDING_SIGN_TIMEOUT};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// A deadline `PENDING_SIGN_TIMEOUT` into the future — what a park site
    /// computes for a local/NIP-46 signer via `sign_deadline_for` /
    /// `active_sign_deadline`.
    fn fresh_deadline() -> Instant {
        Instant::now() + PENDING_SIGN_TIMEOUT
    }

    /// Minimal valid `SignedEvent` for exercising the success poll path.
    fn make_signed_event() -> SignedEvent {
        SignedEvent {
            id: "00".repeat(32),
            sig: "00".repeat(64),
            unsigned: UnsignedEvent {
                pubkey: "11".repeat(32),
                kind: 1,
                tags: vec![],
                content: "parked-op test".to_string(),
                created_at: 0,
            },
        }
    }

    // ── Publish sink (was `PendingSign`) ─────────────────────────────────

    /// A `Pending` publish op keeps across ticks, then resolves into a
    /// [`PublishObligation::Publish`] carrying the parked routing fields.
    #[test]
    fn publish_sink_resolves_into_publish_obligation() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let mut parked = ParkedOp::publish(
            SignerOp::Pending(rx),
            vec!["p-tag".to_string()],
            PublishTarget::Auto,
            Some("corr-pub".to_string()),
            fresh_deadline(),
        );
        // First tick: still pending — no obligation.
        // (We poll through the drain so `into_result` / keep semantics are
        // exercised, but the drain needs a kernel; assert the op directly.)
        let ParkedOpSink::Publish { op, .. } = &mut parked.sink else {
            panic!("expected Publish sink");
        };
        assert!(op.poll().is_none(), "no value sent yet");
        assert!(!parked.timed_out(), "fresh op is within deadline");

        // Broker turns the request around.
        tx.send(Ok(make_signed_event())).unwrap();
        let ParkedOpSink::Publish { op, .. } = &mut parked.sink else {
            panic!("expected Publish sink");
        };
        let signed = op
            .poll()
            .expect("Some after send")
            .expect("Ok payload carries the signed event");
        assert_eq!(signed.unsigned.content, "parked-op test");
    }

    /// A publish op past its deadline reports timed-out — the loop turns this
    /// into a `Failed` obligation (toast + correlation-id failure).
    #[test]
    fn publish_sink_tracks_deadline() {
        let (_tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let overdue = ParkedOp {
            sink: ParkedOpSink::Publish {
                op: SignerOp::Pending(rx),
                p_tags: vec![],
                target: PublishTarget::Auto,
                correlation_id_override: None,
            },
            deadline: Instant::now() - Duration::from_millis(1),
        };
        assert!(overdue.timed_out(), "a past-deadline publish op is timed out");
    }

    // ── SignedEventsProjection sink (was `PendingSignReturn::new`) ────────

    /// `signed_events_projection` parks under the projection sink with the
    /// supplied correlation id.
    #[test]
    fn signed_events_projection_sink_shape() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let parked =
            ParkedOp::signed_events_projection(SignerOp::Pending(rx), "corr-1".to_string(), fresh_deadline());
        assert!(
            matches!(
                &parked.sink,
                ParkedOpSink::SignedEventsProjection { correlation_id, .. }
                    if correlation_id == "corr-1"
            ),
            "signed_events_projection must default to the projection sink"
        );
        drop(tx);
    }

    // ── SignContinuation sink (was `PendingSignReturn::with_continuation`) ─

    /// The boxed sign continuation can be `.take()`n out of the `&mut` sink and
    /// invoked exactly once with a resolved `SignedEvent` (the drain invariant).
    #[test]
    fn sign_continuation_sink_take_then_call() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let captured: Arc<Mutex<Option<Result<SignedEvent, String>>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&captured);
        let mut parked = ParkedOp::sign_continuation(
            SignerOp::Pending(rx),
            SignContinuation::new(move |outcome| {
                *slot.lock().unwrap() = Some(outcome);
            }),
            fresh_deadline(),
        );
        tx.send(Ok(make_signed_event())).unwrap();
        let ParkedOpSink::SignContinuation { op, continuation } = &mut parked.sink else {
            panic!("expected a SignContinuation sink");
        };
        let resolved = op.poll().expect("Some after send").expect("Ok payload");
        continuation
            .take()
            .expect("continuation present until taken")
            .call(Ok(resolved));
        let got = captured.lock().unwrap().take().expect("continuation ran");
        assert_eq!(got.expect("Ok outcome").unsigned.content, "parked-op test");
    }

    // ── CipherContinuation sink (ADR-0050 §D1, new) ──────────────────────

    /// The cipher sink parks a `SignerOp<String>` and delivers the resolved
    /// ciphertext / plaintext to the boxed cipher continuation.
    #[test]
    fn cipher_continuation_sink_take_then_call() {
        let (tx, rx) = mpsc::channel::<Result<String, SignerError>>();
        let captured: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&captured);
        let mut parked = ParkedOp::cipher_continuation(
            SignerOp::Pending(rx),
            CipherContinuation::new(move |outcome| {
                *slot.lock().unwrap() = Some(outcome);
            }),
            fresh_deadline(),
        );
        assert!(
            matches!(&parked.sink, ParkedOpSink::CipherContinuation { .. }),
            "cipher_continuation must park under the cipher sink"
        );
        tx.send(Ok("deadbeefcipher".to_string())).unwrap();
        let ParkedOpSink::CipherContinuation { op, continuation } = &mut parked.sink else {
            panic!("expected a CipherContinuation sink");
        };
        let resolved = op.poll().expect("Some after send").expect("Ok ciphertext");
        continuation
            .take()
            .expect("continuation present")
            .call(Ok(resolved));
        let got = captured.lock().unwrap().take().expect("continuation ran");
        assert_eq!(got.expect("Ok outcome"), "deadbeefcipher");
    }

    /// A cipher broker error / disconnect surfaces as `Some(Err(..))` from
    /// `poll()` so the continuation can run its failure terminal (D6).
    #[test]
    fn cipher_op_disconnect_polls_to_backend_error() {
        let (tx, rx) = mpsc::channel::<Result<String, SignerError>>();
        let mut op: SignerOp<String> = SignerOp::Pending(rx);
        drop(tx); // broker died before responding.
        assert!(
            matches!(op.poll(), Some(Err(SignerError::Backend(_)))),
            "a disconnected cipher channel must poll to Some(Err(Backend))"
        );
    }
}
