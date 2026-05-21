//! `PendingSign` — an in-flight remote (NIP-46) sign operation parked on the
//! actor loop.
//!
//! Background: `sign_active` (`commands/identity.rs`) blocks the actor thread
//! for up to `REMOTE_SIGN_TIMEOUT` (5s) waiting on a NIP-46 broker via
//! `SignerOp::wait`. While it blocks, relay ingest, subscription management,
//! and UI emits all stall — a D8 violation (no polling / no blocking the
//! actor).
//!
//! The fix: the publish path signs through `sign_active_nonblocking`, which
//! hands back the raw `SignerOp` instead of blocking. A local signer's op is
//! `Ready` and resolves on the spot; a remote signer's op is `Pending` and is
//! stashed here. The actor's idle section then `poll()`s every parked
//! `PendingSign` once per loop tick — non-blocking `try_recv` — and publishes
//! the signed event the moment the broker turns the request around.
//!
//! `deadline` bounds the wait: a broker that never responds within
//! `PENDING_SIGN_TIMEOUT` has its `PendingSign` dropped and a toast surfaced
//! (D6 — the error becomes kernel state, the actor never wedges).

use crate::publish::PublishTarget;
use crate::substrate::SignedEvent;
use nmp_signer_iface::SignerOp;
use std::time::{Duration, Instant};

/// Wall-clock budget for a parked remote-sign op. Mirrors the old blocking
/// `REMOTE_SIGN_TIMEOUT` (5s) — long enough for a fast / auto-approving
/// bunker, short enough that a crashed broker cannot strand the publish.
pub(crate) const PENDING_SIGN_TIMEOUT: Duration = Duration::from_secs(5);

/// A remote-sign operation parked on the actor loop, awaiting the broker's
/// kind:24133 response.
pub(crate) struct PendingSign {
    /// The in-flight signer op. `poll()`ed once per idle tick.
    pub op: SignerOp<SignedEvent>,
    /// The `p_tags` to forward to `Kernel::publish_signed` once the signed
    /// event lands. Empty for every current publish callsite (the publish
    /// engine resolves NIP-65 outbox relays itself); carried so the field
    /// can route p-tagged publishes without another signature change.
    pub p_tags: Vec<String>,
    /// D3 routing mode for the publish that fires once the broker turns the
    /// sign request around. `Auto` (the [`Self::new`] default) routes via the
    /// NIP-65 outbox resolver — every kind:1/3/7 publish path. `Explicit` is
    /// the host-pinned opt-out used by [`Self::with_target`]: a NIP-29 group
    /// action must reach the group's own relays, not the author's outbox, so
    /// the target has to survive the remote-sign park (otherwise a bunker
    /// user's group event would silently fall back to the wrong relay set).
    pub target: PublishTarget,
    /// Action correlation_id to report in `last_action_result` once the parked
    /// publish settles, when it differs from the eventual event id. Set on the
    /// `PublishNote` dispatch path: the host received a registry-minted id
    /// before this remote-sign op was parked, and the event id is only known
    /// once the broker returns the signed event. Without carrying it here a
    /// bunker user's dispatched `PublishNote` would settle under the event id
    /// and the host spinner could never be cleared. `None` for every other
    /// parked publish (`react`, `follow`, NIP-29 group actions, …).
    pub correlation_id_override: Option<String>,
    /// Drop-dead time. Past this, the op is abandoned with a toast.
    pub deadline: Instant,
}

impl PendingSign {
    /// Park a sign op whose publish routes via the NIP-65 outbox resolver
    /// (`PublishTarget::Auto`) — the back-compat path every kind:1/3/7
    /// publish handler uses.
    pub fn new(op: SignerOp<SignedEvent>, p_tags: Vec<String>) -> Self {
        Self::with_target(op, p_tags, PublishTarget::Auto)
    }

    /// Park a sign op whose publish routes to an EXPLICIT relay set
    /// (`PublishTarget::Explicit`). Used by host-pinned action executors
    /// (e.g. NIP-29 group actions) so the relay pin survives a remote-signer
    /// round-trip — the idle-tick poll loop publishes through
    /// `Kernel::publish_signed_to` with this exact target.
    pub fn with_target(
        op: SignerOp<SignedEvent>,
        p_tags: Vec<String>,
        target: PublishTarget,
    ) -> Self {
        Self {
            op,
            p_tags,
            target,
            correlation_id_override: None,
            deadline: Instant::now() + PENDING_SIGN_TIMEOUT,
        }
    }

    /// Park a sign op (NIP-65 `Auto` routing) that carries an action
    /// `correlation_id` to report once the publish settles. Used by the
    /// `PublishNote` dispatch path so a bunker user's dispatched note settles
    /// under the registry-minted id the host is waiting on, not the event id.
    pub fn with_correlation_id(
        op: SignerOp<SignedEvent>,
        p_tags: Vec<String>,
        correlation_id_override: Option<String>,
    ) -> Self {
        Self {
            op,
            p_tags,
            target: PublishTarget::Auto,
            correlation_id_override,
            deadline: Instant::now() + PENDING_SIGN_TIMEOUT,
        }
    }

    /// True once the op has overrun `PENDING_SIGN_TIMEOUT`.
    pub fn timed_out(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the parked remote-sign path. These pin the *async*
    //! `SignerOp::Pending` behaviour the actor loop relies on — distinct
    //! from `remote_signer_tests.rs`, whose `StubSigner` always returns a
    //! ready-now op so the `PendingSign` queue never accumulates.
    use super::*;
    use crate::substrate::{SignedEvent, UnsignedEvent};
    use nmp_signer_iface::{SignerError, SignerOp};
    use std::sync::mpsc;

    /// Minimal valid `SignedEvent` for exercising the success poll path.
    fn make_signed_event() -> SignedEvent {
        SignedEvent {
            id: "00".repeat(32),
            sig: "00".repeat(64),
            unsigned: UnsignedEvent {
                pubkey: "11".repeat(32),
                kind: 1,
                tags: vec![],
                content: "pending-sign test".to_string(),
                created_at: 0,
            },
        }
    }

    /// A `Pending` op returns `None` from `poll()` until the broker responds.
    /// This is the non-blocking property the actor loop depends on: the
    /// idle-tick `retain_mut` keeps the `PendingSign` alive without stalling.
    #[test]
    fn poll_returns_none_while_pending() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let mut ps = PendingSign::new(SignerOp::Pending(rx), vec![]);
        assert!(
            ps.op.poll().is_none(),
            "Pending op must poll to None before the sender produces a value"
        );
        assert!(
            !ps.timed_out(),
            "a freshly-created PendingSign is well within its deadline"
        );
        drop(tx); // disconnect — no value was ever sent.
    }

    /// Once the broker sends a successful result, a later `poll()` resolves
    /// it. Mirrors the actor loop seeing `Some(Ok(signed))` on a later tick.
    #[test]
    fn poll_resolves_with_signed_event_after_send() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let mut ps = PendingSign::new(SignerOp::Pending(rx), vec!["p-tag".to_string()]);

        // First tick: still pending.
        assert!(ps.op.poll().is_none(), "no value sent yet");

        // Broker turns the request around.
        tx.send(Ok(make_signed_event())).unwrap();

        // Next tick: the signed event is delivered.
        let signed = ps
            .op
            .poll()
            .expect("poll must yield Some after the sender produces a value")
            .expect("the result carries the signed event, not an error");
        assert_eq!(signed.unsigned.content, "pending-sign test");
        // p_tags ride alongside the op for the publish callsite.
        assert_eq!(ps.p_tags, vec!["p-tag".to_string()]);
    }

    /// A broker-side rejection surfaces through `poll()` as `Some(Err(..))`.
    /// Mirrors the actor loop's `Some(Err(e))` branch.
    #[test]
    fn poll_resolves_with_error_after_send() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let mut ps = PendingSign::new(SignerOp::Pending(rx), vec![]);

        tx.send(Err(SignerError::Rejected("user said no".to_string())))
            .unwrap();

        let result = ps.op.poll();
        assert!(
            matches!(result, Some(Err(SignerError::Rejected(_)))),
            "a rejected sign must poll to Some(Err(Rejected)), got {result:?}"
        );
    }

    /// A dropped sender (broker channel torn down without a value) surfaces
    /// as `Some(Err(Backend(..)))` — the op never strands the actor loop.
    #[test]
    fn poll_resolves_with_backend_error_on_disconnect() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let mut ps = PendingSign::new(SignerOp::Pending(rx), vec![]);

        drop(tx); // broker died before responding.

        let result = ps.op.poll();
        assert!(
            matches!(result, Some(Err(SignerError::Backend(_)))),
            "a disconnected channel must poll to Some(Err(Backend)), got {result:?}"
        );
    }

    /// `timed_out()` is false before the deadline and true after it. A
    /// deadline set in the past reports timed-out immediately — this is the
    /// signal the actor loop uses to abandon a non-responsive broker.
    #[test]
    fn timed_out_tracks_the_deadline() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();

        // Fresh op: deadline is PENDING_SIGN_TIMEOUT in the future.
        let fresh = PendingSign::new(SignerOp::Pending(rx), vec![]);
        assert!(!fresh.timed_out(), "a fresh PendingSign has not timed out");

        // Op whose deadline already elapsed.
        let (_tx2, rx2) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let overdue = PendingSign {
            op: SignerOp::Pending(rx2),
            p_tags: vec![],
            target: PublishTarget::Auto,
            correlation_id_override: None,
            deadline: Instant::now() - Duration::from_millis(1),
        };
        assert!(
            overdue.timed_out(),
            "a PendingSign past its deadline reports timed_out"
        );
        drop(tx);
    }
}
