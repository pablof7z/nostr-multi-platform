//! The target-agnostic parked-signer-op queue + canonical drain driver
//! (ADR-0050 §D2; issue #1753 S6).
//!
//! Before #1753 the `Vec<ParkedOp>` lived as a bare local in the native actor
//! loop, and the `retain_mut` that drives [`super::resolve_parked_op`] over it
//! was inlined there. That made the drain *driver* native-only: the wasm
//! `KernelReducer` (which has no actor loop and no idle tick) had no way to own
//! parked signer ops or run the same drain.
//!
//! [`ParkedSignerOps`] hoists both — the queue **and** its one `retain_mut`
//! drive — into a component that BOTH targets share:
//!
//! * **Native** (`actor/mod.rs`): the loop holds one `ParkedSignerOps`, pushes
//!   parked ops into it from the dispatch arms, and calls [`ParkedSignerOps::drive`]
//!   once per idle tick. The collected [`PublishObligation`] / [`AuthObligation`]
//!   are handed back to the loop, which owns relay routing exactly as before.
//!   D8: `drive` polls each op with a non-blocking `SignerOp::poll`; the
//!   per-op deadline is the wall-clock timeout gate — never a sleep or a wait.
//!
//! * **Wasm** (`KernelReducer`, issue #1753): the reducer holds one
//!   `ParkedSignerOps`. A NIP-07 sign verb emits a capability request and parks
//!   a [`super::ParkedOp::sign_continuation`]; when the main-thread JS bridge
//!   posts the signed bytes back as a `DeliverSignerResponse` message, the
//!   reducer hands the value to the op (closing its channel) and calls `drive`
//!   **once, from that inbound-message handler** — pure message re-entry. There
//!   is NO timer, NO poll loop, NO blocking `SignerOp::wait` anywhere in the
//!   wasm completion path (D8): completion is noticed because the message
//!   arrived, not because something polled for it.
//!
//! This is the SAME mechanism the native NIP-46 broker uses: the broker resolves
//! the parked op's channel out of band, and the drain picks the value up with a
//! single `poll`. #1753 reuses that one drain; it does not add a parallel wasm
//! copy. The only difference is *what drives the single drive call* — the native
//! idle tick vs. the wasm inbound message.

use super::drain::resolve_parked_op;
use super::sinks::{AuthObligation, ParkedOp, PublishObligation};

/// The obligations a single [`ParkedSignerOps::drive`] pass collected for its
/// caller to execute. The drain settles projection / continuation sinks against
/// the kernel directly; the `Publish` and `Auth` sinks return routing
/// obligations because relay routing is the caller's concern (the native loop
/// owns the pool; the wasm reducer keeps web publish disabled, §honest-disable
/// gate, so it simply never parks a `Publish` op).
pub(crate) struct DrainBatch {
    /// Publish-routing obligations from resolved `Publish` sinks.
    pub publish: Vec<PublishObligation>,
    /// NIP-42 AUTH-routing obligations from resolved `Auth` sinks (V-06 / #960).
    pub auth: Vec<AuthObligation>,
    /// `true` when at least one op resolved and changed kernel state this pass,
    /// so the caller emits a snapshot now rather than waiting for `flush_due`.
    pub changed: bool,
}

/// The shared parked-signer-op queue. Owns one `Vec<ParkedOp>` and the canonical
/// `retain_mut` drain driver.
#[derive(Default)]
pub(crate) struct ParkedSignerOps {
    ops: Vec<ParkedOp>,
}

impl ParkedSignerOps {
    /// A fresh, empty queue.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self { ops: Vec::new() }
    }

    /// Park one op.
    pub(crate) fn push(&mut self, op: ParkedOp) {
        self.ops.push(op);
    }

    /// `true` when no op is parked — the caller skips the drive entirely so an
    /// idle tick is a heap-free zero-item check (D8).
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Number of parked ops. Read by the wasm round-trip diagnostics
    /// (`KernelReducer::pending_sign_roundtrips`) and test assertions.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.ops.len()
    }

    /// Consume the queue, yielding its parked ops. Test-only seam: the signer-
    /// port dispatch tests build a queue through `dispatch_one`, then resolve /
    /// assert against the raw ops (indexing `[0]`, calling `resolve_parked_op`
    /// directly). Production drives through [`Self::drive`] instead.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn into_vec(self) -> Vec<ParkedOp> {
        self.ops
    }

    /// The ONE canonical drain pass: `retain_mut` over the queue, resolving each
    /// op against `kernel` via [`resolve_parked_op`] and collecting the routing
    /// obligations. Resolved / errored / timed-out ops are dropped; still-pending
    /// ops are kept. Returns the obligations the caller must execute.
    ///
    /// Called once per native idle tick AND once per wasm `DeliverSignerResponse`
    /// message — one driver, two drive sites, no parallel copy. D8: every op is
    /// polled exactly once per call with a non-blocking `SignerOp::poll`; the
    /// deadline is the wall-clock gate.
    pub(crate) fn drive(&mut self, kernel: &mut crate::kernel::Kernel) -> DrainBatch {
        let mut publish = Vec::new();
        let mut auth = Vec::new();
        let mut changed = false;
        self.ops.retain_mut(|parked| {
            let outcome = resolve_parked_op(parked, kernel);
            if let Some(obligation) = outcome.publish {
                publish.push(obligation);
            }
            if let Some(obligation) = outcome.auth {
                auth.push(obligation);
            }
            changed |= outcome.changed;
            outcome.keep
        });
        DrainBatch {
            publish,
            auth,
            changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::SignContinuation;
    use crate::substrate::{SignedEvent, UnsignedEvent};
    use crate::time::Instant;
    use nmp_signer_iface::{SignerError, SignerOp, PENDING_SIGN_TIMEOUT};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn signed_event() -> SignedEvent {
        SignedEvent {
            id: "00".repeat(32),
            sig: "00".repeat(64),
            unsigned: UnsignedEvent {
                pubkey: "11".repeat(32),
                kind: 1,
                tags: vec![],
                content: "queue test".to_string(),
                created_at: 0,
            },
        }
    }

    /// A still-pending op is kept across a `drive` pass and reports no change —
    /// the queue does not drop or resolve it until its channel produces a value.
    #[test]
    fn pending_op_is_kept_and_reports_no_change() {
        let mut kernel = crate::kernel::Kernel::new(64);
        let mut queue = ParkedSignerOps::new();
        let (_tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let captured: Arc<Mutex<Option<Result<SignedEvent, String>>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&captured);
        queue.push(ParkedOp::sign_continuation(
            SignerOp::Pending(rx),
            SignContinuation::new(move |o| *slot.lock().unwrap() = Some(o)),
            Instant::now() + PENDING_SIGN_TIMEOUT,
        ));

        let batch = queue.drive(&mut kernel);
        assert!(!batch.changed, "a pending op changes nothing");
        assert_eq!(queue.len(), 1, "a pending op is kept");
        assert!(
            captured.lock().unwrap().is_none(),
            "the continuation must not run while the op is pending"
        );
    }

    /// Once the channel produces a value, a single `drive` pass resolves the op,
    /// runs its continuation with the signed event, and drops it from the queue.
    /// This is the wasm message-re-entry shape: the value is delivered out of
    /// band (here via `tx.send`, in production via the JS bridge), then exactly
    /// ONE `drive` call — no loop — settles it.
    #[test]
    fn delivered_value_resolves_continuation_in_one_drive() {
        let mut kernel = crate::kernel::Kernel::new(64);
        let mut queue = ParkedSignerOps::new();
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let captured: Arc<Mutex<Option<Result<SignedEvent, String>>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&captured);
        queue.push(ParkedOp::sign_continuation(
            SignerOp::Pending(rx),
            SignContinuation::new(move |o| *slot.lock().unwrap() = Some(o)),
            Instant::now() + PENDING_SIGN_TIMEOUT,
        ));

        // The "message" arrives: deliver the signed value into the op.
        tx.send(Ok(signed_event())).unwrap();

        // ONE drive call — message-triggered, not a poll loop.
        let batch = queue.drive(&mut kernel);
        assert!(
            batch.changed,
            "the resolved op changed kernel-observable state"
        );
        assert!(
            queue.is_empty(),
            "the resolved op is dropped from the queue"
        );
        let got = captured.lock().unwrap().take().expect("continuation ran");
        assert_eq!(got.expect("Ok").unsigned.content, "queue test");
    }

    /// A past-deadline op resolves to a timeout error terminal on the next drive
    /// (the wall-clock gate, D8) without any sleep.
    #[test]
    fn overdue_op_times_out_on_drive() {
        let mut kernel = crate::kernel::Kernel::new(64);
        let mut queue = ParkedSignerOps::new();
        let (_tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let captured: Arc<Mutex<Option<Result<SignedEvent, String>>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&captured);
        queue.push(ParkedOp::sign_continuation(
            SignerOp::Pending(rx),
            SignContinuation::new(move |o| *slot.lock().unwrap() = Some(o)),
            Instant::now() - Duration::from_millis(1),
        ));
        let batch = queue.drive(&mut kernel);
        assert!(batch.changed);
        assert!(queue.is_empty(), "a timed-out op is dropped");
        let got = captured.lock().unwrap().take().expect("continuation ran");
        assert!(
            got.is_err(),
            "a timed-out sign resolves the continuation with Err"
        );
    }
}
