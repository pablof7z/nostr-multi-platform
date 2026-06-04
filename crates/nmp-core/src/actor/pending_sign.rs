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

use super::SignContinuation;
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
    /// Action `correlation_id` to report in `action_results` once the parked
    /// publish settles, when it differs from the eventual event id. Set on the
    /// `PublishRaw` dispatch path: the host received a registry-minted id
    /// before this remote-sign op was parked, and the event id is only known
    /// once the broker returns the signed event. Without carrying it here a
    /// bunker user's dispatched `PublishRaw` would settle under the event id
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
    #[must_use]
    pub fn new(op: SignerOp<SignedEvent>, p_tags: Vec<String>) -> Self {
        Self::with_target(op, p_tags, PublishTarget::Auto)
    }

    /// Park a sign op whose publish routes to an EXPLICIT relay set
    /// (`PublishTarget::Explicit`). Used by host-pinned action executors
    /// (e.g. NIP-29 group actions) so the relay pin survives a remote-signer
    /// round-trip — the idle-tick poll loop publishes through
    /// `Kernel::publish_signed_to` with this exact target.
    #[must_use]
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
    /// `PublishRaw` dispatch path so a bunker user's dispatched note settles
    /// under the registry-minted id the host is waiting on, not the event id.
    #[must_use]
    pub fn with_correlation_id(
        op: SignerOp<SignedEvent>,
        p_tags: Vec<String>,
        correlation_id_override: Option<String>,
    ) -> Self {
        Self::with_target_and_correlation_id(
            op,
            p_tags,
            PublishTarget::Auto,
            correlation_id_override,
        )
    }

    /// Park a sign op with both a routing target and an optional action
    /// correlation id. This is the lossless path for dispatched publishes
    /// whose `PublishTarget` is not always `Auto`.
    #[must_use]
    pub fn with_target_and_correlation_id(
        op: SignerOp<SignedEvent>,
        p_tags: Vec<String>,
        target: PublishTarget,
        correlation_id_override: Option<String>,
    ) -> Self {
        Self {
            op,
            p_tags,
            target,
            correlation_id_override,
            deadline: Instant::now() + PENDING_SIGN_TIMEOUT,
        }
    }

    /// True once the op has overrun `PENDING_SIGN_TIMEOUT`.
    pub fn timed_out(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// Where a resolved [`PendingSignReturn`] delivers its signed event.
///
/// Two terminal sinks share one park/drain path so the idle-loop machinery is
/// not duplicated:
///
/// * [`Self::SignedEventsProjection`] — the original
///   [`crate::actor::ActorCommand::SignEventForReturn`] behaviour: the signed
///   JSON (or error) is written to the `signed_events` snapshot projection
///   keyed by `correlation_id`. The host reads the projection once and
///   attaches the signed event to an out-of-band transport.
/// * [`Self::Continuation`] — the generic
///   [`crate::actor::ActorCommand::SignEventForAccount`] backend-transparent
///   sign port: the boxed continuation is invoked with the resolved
///   `SignedEvent` (or an error string). The continuation runs on the actor
///   thread (inline for a local nsec, from the idle-loop drain for a parked
///   NIP-46 bunker) and may only enqueue further work (e.g. spawn an HTTP
///   worker) — never block.
///
/// The continuation is held inside an `Option` so the idle-loop drain — which
/// borrows each parked op as `&mut` via `retain_mut` — can `.take()` it before
/// calling (an `FnOnce` cannot be invoked through `&mut`).
pub(crate) enum PendingSignReturnSink {
    /// Write the signed JSON / error into `signed_events[correlation_id]`.
    SignedEventsProjection { correlation_id: String },
    /// Invoke the boxed continuation with the resolved sign outcome.
    Continuation(Option<SignContinuation>),
}

/// A remote-sign operation parked for sign-and-return (no publish).
///
/// Sibling of [`PendingSign`], which always routes the signed event into the
/// publish engine. `PendingSignReturn` covers BOTH the
/// [`crate::actor::ActorCommand::SignEventForReturn`] seam (sink =
/// [`PendingSignReturnSink::SignedEventsProjection`]) and the generic
/// [`crate::actor::ActorCommand::SignEventForAccount`] backend-transparent
/// sign port (sink = [`PendingSignReturnSink::Continuation`]). The signed
/// event NEVER reaches the publish engine on either path — the sink decides
/// the terminal.
///
/// Same non-blocking contract as `PendingSign` (D8): a local nsec resolves its
/// `SignerOp` immediately (so this struct is never parked for it), and a
/// NIP-46 bunker's `SignerOp::Pending` is `poll()`ed once per idle tick until
/// the broker responds or `PENDING_SIGN_TIMEOUT` elapses.
pub(crate) struct PendingSignReturn {
    /// The in-flight signer op. `poll()`ed once per idle tick.
    pub op: SignerOp<SignedEvent>,
    /// Where the resolved (or timed-out / errored) outcome is delivered.
    pub sink: PendingSignReturnSink,
    /// Drop-dead time. Past this, the op is abandoned and an error outcome is
    /// delivered to the sink so the host's continuation never hangs.
    pub deadline: Instant,
}

impl PendingSignReturn {
    /// Park a sign-and-return op whose resolved outcome lands in the
    /// `signed_events` projection under `correlation_id` (the
    /// `SignEventForReturn` seam). Deadlined `PENDING_SIGN_TIMEOUT` (5s) into
    /// the future — identical budget to [`PendingSign::new`].
    #[must_use]
    pub fn new(op: SignerOp<SignedEvent>, correlation_id: String) -> Self {
        Self {
            op,
            sink: PendingSignReturnSink::SignedEventsProjection { correlation_id },
            deadline: Instant::now() + PENDING_SIGN_TIMEOUT,
        }
    }

    /// Park a sign-and-return op whose resolved outcome is handed to a boxed
    /// continuation (the generic `SignEventForAccount` port). Same 5s budget.
    #[must_use]
    pub fn with_continuation(op: SignerOp<SignedEvent>, continuation: SignContinuation) -> Self {
        Self {
            op,
            sink: PendingSignReturnSink::Continuation(Some(continuation)),
            deadline: Instant::now() + PENDING_SIGN_TIMEOUT,
        }
    }

    /// True once the op has overrun `PENDING_SIGN_TIMEOUT`.
    pub fn timed_out(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// Resolve one parked [`PendingSignReturn`] against its sink. Called once per
/// idle tick from the actor loop's `pending_sign_returns` drain (D8 — a single
/// non-blocking `poll()`, never a wait).
///
/// Returns `true` to KEEP the op (still pending, broker has not responded and
/// the deadline has not elapsed) or `false` once it has resolved / errored /
/// timed-out so `retain_mut` drops it.
///
/// All three outcomes (signed / broker error / timeout) collapse into one
/// `Result<SignedEvent, String>` so the sink dispatch is identical for BOTH the
/// `signed_events` projection sink AND the generic continuation port — neither
/// may silently drop its terminal (D6: a dropped continuation hangs the host
/// spinner forever). The caller is responsible for emitting a snapshot after a
/// `false` return.
pub(crate) fn resolve_pending_sign_return(
    ps: &mut PendingSignReturn,
    kernel: &mut crate::kernel::Kernel,
) -> bool {
    let outcome: Result<SignedEvent, String> = match ps.op.poll() {
        None => {
            if ps.timed_out() {
                Err("signing timed out".to_string())
            } else {
                return true; // Still pending — keep for the next tick.
            }
        }
        Some(Ok(signed)) => Ok(signed),
        Some(Err(e)) => Err(e.to_string()),
    };

    match &mut ps.sink {
        PendingSignReturnSink::SignedEventsProjection { correlation_id } => {
            // D13 `SignEventForReturn`: write the signed JSON / error into the
            // `signed_events[correlation_id]` projection.
            let recorded =
                outcome.map(|signed| crate::actor::dispatch::signed_event_to_json(&signed));
            kernel.record_signed_event_return(correlation_id, recorded);
        }
        PendingSignReturnSink::Continuation(slot) => {
            // Generic `SignEventForAccount` port: take the boxed continuation
            // out of the `&mut` sink (an `FnOnce` cannot be called through
            // `&mut`) and invoke it with the resolved outcome. The continuation
            // runs on the actor thread and only enqueues further work (D8). On
            // `Err` it must itself resolve the host's action terminal.
            if let Some(continuation) = slot.take() {
                continuation.call(outcome);
            }
        }
    }
    false // Done — remove.
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

    // ── PendingSignReturn (D13 sign-and-return) ──────────────────────────
    // The sign-and-return park mirrors `PendingSign`'s non-blocking poll
    // contract, minus the publish routing. These tests pin the same
    // Pending → resolve / reject / disconnect / timeout transitions the
    // actor idle loop's `pending_sign_returns` drain depends on.

    /// A `Pending` return-op polls to `None` until the broker responds, and a
    /// freshly created park is within its deadline.
    #[test]
    fn return_poll_returns_none_while_pending() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let mut ps = PendingSignReturn::new(SignerOp::Pending(rx), "corr-1".to_string());
        assert!(
            ps.op.poll().is_none(),
            "a Pending return-op polls to None before the broker responds"
        );
        assert!(
            !ps.timed_out(),
            "a fresh PendingSignReturn is within deadline"
        );
        assert!(
            matches!(
                &ps.sink,
                PendingSignReturnSink::SignedEventsProjection { correlation_id }
                    if correlation_id == "corr-1"
            ),
            "PendingSignReturn::new must default to the signed_events projection sink"
        );
        drop(tx);
    }

    /// The signed event is delivered on a later poll once the broker sends it —
    /// the success branch the idle loop serializes into `signed_events`.
    #[test]
    fn return_poll_resolves_with_signed_event_after_send() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let mut ps = PendingSignReturn::new(SignerOp::Pending(rx), "corr-2".to_string());
        assert!(ps.op.poll().is_none(), "no value sent yet");
        tx.send(Ok(make_signed_event())).unwrap();
        let signed = ps
            .op
            .poll()
            .expect("poll yields Some after the broker sends")
            .expect("the result carries the signed event");
        assert_eq!(signed.unsigned.content, "pending-sign test");
    }

    /// A broker rejection surfaces as `Some(Err(..))` — the failure branch the
    /// idle loop records as an error verdict under the correlation id.
    #[test]
    fn return_poll_resolves_with_error_after_send() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let mut ps = PendingSignReturn::new(SignerOp::Pending(rx), "corr-3".to_string());
        tx.send(Err(SignerError::Rejected("user said no".to_string())))
            .unwrap();
        assert!(
            matches!(ps.op.poll(), Some(Err(SignerError::Rejected(_)))),
            "a rejected return-sign polls to Some(Err(Rejected))"
        );
    }

    /// A return-op past its deadline reports timed-out — the idle loop abandons
    /// it and records a `"signing timed out"` verdict so the host never hangs.
    #[test]
    fn return_timed_out_tracks_the_deadline() {
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let fresh = PendingSignReturn::new(SignerOp::Pending(rx), "corr-4".to_string());
        assert!(
            !fresh.timed_out(),
            "a fresh PendingSignReturn has not timed out"
        );

        let (_tx2, rx2) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let overdue = PendingSignReturn {
            op: SignerOp::Pending(rx2),
            sink: PendingSignReturnSink::SignedEventsProjection {
                correlation_id: "corr-5".to_string(),
            },
            deadline: Instant::now() - Duration::from_millis(1),
        };
        assert!(
            overdue.timed_out(),
            "a PendingSignReturn past its deadline reports timed_out"
        );
        drop(tx);
    }

    // ── PendingSignReturnSink::Continuation (generic SignEventForAccount) ──
    // The continuation sink shares the same park/deadline machinery as the
    // signed_events sink. These pin the `.take()`-then-`call()` invariant the
    // idle-loop drain relies on (an `FnOnce` cannot be called through `&mut`).

    /// `with_continuation` parks under the `Continuation` sink, and the boxed
    /// continuation can be `.take()`n out and invoked exactly once with a
    /// resolved `SignedEvent`.
    #[test]
    fn continuation_sink_invokes_with_signed_event() {
        use std::sync::{Arc, Mutex};
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let captured: Arc<Mutex<Option<Result<SignedEvent, String>>>> = Arc::new(Mutex::new(None));
        let sink_slot = Arc::clone(&captured);
        let mut ps = PendingSignReturn::with_continuation(
            SignerOp::Pending(rx),
            SignContinuation::new(move |outcome| {
                *sink_slot.lock().unwrap() = Some(outcome);
            }),
        );
        assert!(ps.op.poll().is_none(), "Pending before the broker responds");
        tx.send(Ok(make_signed_event())).unwrap();
        let resolved = ps.op.poll().expect("Some after send").expect("Ok payload");

        // Drain-site invariant: take the continuation out of the &mut sink,
        // then call it (mirrors the idle-loop `retain_mut` drain).
        let PendingSignReturnSink::Continuation(slot) = &mut ps.sink else {
            panic!("expected a Continuation sink");
        };
        slot.take()
            .expect("continuation present until taken")
            .call(Ok(resolved));

        let got = captured.lock().unwrap().take().expect("continuation ran");
        let signed = got.expect("Ok outcome");
        assert_eq!(signed.unsigned.content, "pending-sign test");
    }

    /// A timeout / broker error must invoke the continuation with `Err(_)` so
    /// the downstream action terminal still resolves (D6 — no stuck spinner).
    #[test]
    fn continuation_sink_invokes_with_error_outcome() {
        use std::sync::{Arc, Mutex};
        let (_tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();
        let captured: Arc<Mutex<Option<Result<SignedEvent, String>>>> = Arc::new(Mutex::new(None));
        let sink_slot = Arc::clone(&captured);
        let mut ps = PendingSignReturn::with_continuation(
            SignerOp::Pending(rx),
            SignContinuation::new(move |outcome| {
                *sink_slot.lock().unwrap() = Some(outcome);
            }),
        );
        let PendingSignReturnSink::Continuation(slot) = &mut ps.sink else {
            panic!("expected a Continuation sink");
        };
        slot.take()
            .expect("continuation present")
            .call(Err("signing timed out".to_string()));
        let got = captured.lock().unwrap().take().expect("continuation ran");
        assert_eq!(got.unwrap_err(), "signing timed out");
    }
}
