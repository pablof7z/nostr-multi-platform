//! The single waking actor inbox (ADR-0050 §D3a).
//!
//! Before this module the actor loop had two bare `std::sync::mpsc` channels:
//! a command channel drained non-blockingly with `try_recv` at the top of each
//! iteration, and a relay channel whose `recv_timeout` was the loop's *only*
//! blocking point. Consequence: sending an [`ActorCommand`] did **not** wake a
//! relay-blocked actor — the command sat for up to the 250 ms `compute_wait`
//! cap whenever no relay traffic flowed (and the same latency afflicted
//! ADR-0040 `CapabilityResultReady` re-entry).
//!
//! This module collapses both into **one** blocking inbox carrying
//! [`ActorMail`]. The loop blocks on a single [`Inbox::recv_timeout`]; any
//! mail — command *or* relay — wakes it. Command-lane priority is preserved
//! exactly: [`MailScheduler`] classifies received mail into two local lanes and
//! always serves the command lane first, up to the
//! [`COMMAND_DRAIN_BUDGET`](super::fairness::COMMAND_DRAIN_BUDGET), before
//! relay work — identical to the prior `try_recv`-burst semantics, just driven
//! off one channel instead of two.
//!
//! D8: there is still exactly **one** blocking wait per loop iteration
//! ([`Inbox::recv_timeout`]); no sleeps, no polling, no second mechanism.
//! D0: `ActorMail` / `CommandSender` are substrate-generic transport types —
//! they name no protocol concept.

#[cfg(feature = "native")]
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SendError, Sender, TryRecvError};

#[cfg(all(test, feature = "native"))]
use super::fairness::{CommandDrain, COMMAND_DRAIN_BUDGET};
use super::ActorCommand;
#[cfg(feature = "native")]
use nmp_network::pool::PoolEvent;

/// Hard cap on the actor's local relay backlog (the `VecDeque<PoolEvent>` the
/// [`MailScheduler`] stages between blocking receives).
///
/// Under a sustained relay-event flood — a relay replaying thousands of
/// historical events, say — relay mail can arrive faster than the actor drains
/// it. Without a cap the backlog grows without bound (memory growth) and, paired
/// with the bounded drain below, the actor would otherwise busy-spin one event
/// at a time. The cap bounds memory; on overflow the *oldest* staged event is
/// dropped (D1 tolerates partial state — a dropped relay frame is recoverable
/// via re-subscription / EOSE-driven refetch, and the newest events are the most
/// relevant to keep). Drops are counted ([`MailScheduler::relay_backlog_drops`])
/// so the loss is observable rather than silent.
#[cfg(feature = "native")]
pub(super) const RELAY_BACKLOG_CAP: usize = 512;

/// Maximum number of stashed backlog items [`MailScheduler::next_after_drain`]
/// serves before it MUST fall through to the single blocking `recv_timeout`.
///
/// Serving a *bounded* batch (rather than one-per-call) lets the backlog drain
/// faster than a flood fills it, while always falling through to the one
/// blocking wait once the batch is exhausted preserves D8 (exactly one blocking
/// wait per loop iteration) and kills the busy-spin: a non-empty backlog no
/// longer indefinitely bypasses `recv_timeout`.
#[cfg(feature = "native")]
pub(super) const RELAY_BACKLOG_DRAIN_BATCH: usize = 16;

/// One item the actor inbox carries.
///
/// The [`Relay`](ActorMail::Relay) variant is `native`-only because its payload
/// (`nmp_network::pool::PoolEvent`) lives behind `nmp-network/native`. On
/// `wasm32` / no-`native` builds the inbox carries commands only — there is no
/// relay pool to feed it — so [`CommandSender`] (which the always-compiled
/// `substrate::protocol` seam hands to workers) stays nameable without pulling
/// in the pool surface.
pub enum ActorMail {
    /// A host/worker/self-feedback [`ActorCommand`]. Served on the priority
    /// lane.
    Command(ActorCommand),
    /// A relay event from the pool's translator thread (via `PoolEventSink`).
    #[cfg(feature = "native")]
    Relay(nmp_network::pool::PoolEvent),
}

impl std::fmt::Debug for ActorMail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorMail::Command(cmd) => f.debug_tuple("Command").field(cmd).finish(),
            #[cfg(feature = "native")]
            ActorMail::Relay(_) => f.write_str("Relay(..)"),
        }
    }
}

/// Error returned when an [`ActorCommand`] cannot be delivered because the
/// actor (and therefore its inbox receiver) is gone.
///
/// Carries the undelivered command back to the caller — the same contract as
/// `std::sync::mpsc::SendError<ActorCommand>`, so existing call sites that only
/// observe `.is_err()` / `.expect(..)` / `let _ = ..` are behaviour-preserved.
#[derive(Debug)]
pub struct CommandSendError(pub ActorCommand);

impl std::fmt::Display for CommandSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("sending on a closed actor inbox")
    }
}

impl std::error::Error for CommandSendError {}

/// A cheap, cloneable handle for sending [`ActorCommand`]s into the actor
/// inbox.
///
/// This is the single command-send seam (ADR-0050 §D3a). It replaces the bare
/// `std::sync::mpsc::Sender<ActorCommand>` that used to be handed out to host
/// code, capability/protocol workers, the broker adapter, and the actor's own
/// self-feedback path. Because every send now lands on the *one* inbox the
/// actor blocks on, **any** command send is a genuine wake.
///
/// `send` mirrors `mpsc::Sender::send`: it returns `Ok(())` on success and an
/// error carrying the undelivered command when the actor is gone. The wrapped
/// `Sender<ActorMail>` is `Clone`, so `CommandSender` is too — clones target
/// the same inbox.
#[derive(Clone, Debug)]
pub struct CommandSender {
    tx: Sender<ActorMail>,
}

impl CommandSender {
    /// Wrap an inbox sender. Construction is the only place that knows the
    /// mail type; everything downstream speaks [`ActorCommand`].
    #[must_use]
    pub fn new(tx: Sender<ActorMail>) -> Self {
        Self { tx }
    }

    /// Derive the relay-side sink for the same inbox, to hand to
    /// `Pool::new`. Relay events delivered through it land as
    /// [`ActorMail::Relay`] on the one channel the actor blocks on.
    #[cfg(feature = "native")]
    pub(super) fn relay_sink(&self) -> RelayMailSink {
        RelayMailSink::new(self.tx.clone())
    }

    /// Send a command into the actor inbox, waking the actor.
    ///
    /// On a closed inbox the command is handed back inside
    /// [`CommandSendError`] (mirroring `mpsc::SendError`); the value is not
    /// lost to the caller.
    pub fn send(&self, command: ActorCommand) -> Result<(), CommandSendError> {
        // `send` only ever enqueues `Command` mail, so the error payload (when
        // the inbox is closed) is exactly the command we just tried to send.
        // We recover it with `if let` (never `unreachable!`, D6) and fall back
        // to a no-payload-loss `Shutdown` only on the structurally-impossible
        // relay arm rather than panicking.
        self.tx
            .send(ActorMail::Command(command))
            .map_err(|SendError(mail)| {
                if let ActorMail::Command(cmd) = mail {
                    CommandSendError(cmd)
                } else {
                    CommandSendError(ActorCommand::Shutdown)
                }
            })
    }
}

/// The actor's receiving end of the inbox — the loop's single blocking point.
#[cfg(feature = "native")]
pub(super) struct Inbox {
    rx: Receiver<ActorMail>,
}

#[cfg(feature = "native")]
impl Inbox {
    pub(super) fn new(rx: Receiver<ActorMail>) -> Self {
        Self { rx }
    }

    /// Block up to `timeout` for the next mail. The loop's one wait per
    /// iteration (D8). A timeout falls through to idle work.
    pub(super) fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<ActorMail, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }

    /// Blocking receive used by the bootstrap path (wait for the first
    /// command before constructing the kernel). Returns `None` if the inbox is
    /// closed.
    pub(super) fn recv(&self) -> Option<ActorMail> {
        self.rx.recv().ok()
    }

    /// Non-blocking drain of one mail, if any is queued.
    pub(super) fn try_recv(&self) -> Result<ActorMail, TryRecvError> {
        self.rx.try_recv()
    }
}

/// The relay-side sink the pool's translator thread pushes into. Wraps the
/// inbox sender and tags each [`PoolEvent`](nmp_network::pool::PoolEvent) as
/// [`ActorMail::Relay`] so relay traffic and commands share the one waking
/// channel.
///
/// Send failures are dropped: a gone receiver means the actor is gone, which
/// is exactly the prior bare-`Sender<PoolEvent>` behaviour (the translator
/// stops translating when its workers exit on pool shutdown).
#[cfg(feature = "native")]
#[derive(Clone)]
pub(super) struct RelayMailSink {
    tx: Sender<ActorMail>,
}

#[cfg(feature = "native")]
impl RelayMailSink {
    pub(super) fn new(tx: Sender<ActorMail>) -> Self {
        Self { tx }
    }
}

#[cfg(feature = "native")]
impl nmp_network::pool::PoolEventSink for RelayMailSink {
    fn send_event(&self, event: nmp_network::pool::PoolEvent) {
        let _ = self.tx.send(ActorMail::Relay(event));
    }
}

/// What the actor loop should do next, decided by [`MailScheduler`] after the
/// single blocking receive.
#[cfg(feature = "native")]
pub(super) enum LoopStep {
    /// Dispatch this command through the command-lane path.
    Command(ActorCommand),
    /// Process this relay event through `handle_relay_event`.
    Relay(PoolEvent),
    /// No mail this iteration — fall through to idle work.
    Idle,
    /// The inbox is closed; tear the actor down.
    Shutdown,
}

/// Single-channel lane scheduler that reproduces the old dual-channel
/// command-priority semantics exactly, off one [`ActorMail`] inbox.
///
/// Each loop iteration calls, in order:
///
/// 1. `drain_command_lane` — drain queued mail with non-blocking `try_recv`,
///    dispatching commands first up to [`COMMAND_DRAIN_BUDGET`] and stashing any
///    relay mail seen along the way into the bounded backlog. Stops at the
///    budget (leftover mail stays in the channel for the next iteration) or when
///    the channel is empty.
/// 2. [`next_after_drain`](MailScheduler::next_after_drain) — serve a *bounded
///    batch* of stashed backlog items (up to [`RELAY_BACKLOG_DRAIN_BATCH`], zero
///    wait, relay not starved), then ALWAYS fall through to the single blocking
///    `recv_timeout` once the batch is exhausted (or the backlog is empty). That
///    `recv_timeout` is the loop's *only* wait (D8); a non-empty backlog no
///    longer bypasses it forever, so a sustained flood cannot busy-spin the
///    actor. A command that arrives during the wait is returned for the command
///    path, preserving "command sends wake the actor".
///
/// The backlog is **bounded** at [`RELAY_BACKLOG_CAP`]: relay mail arriving
/// across iterations under a sustained flood accumulates, so on overflow
/// [`stash_relay`](MailScheduler::stash_relay) drops the *oldest* staged event
/// (counted via [`relay_backlog_drops`](MailScheduler::relay_backlog_drops)) to
/// keep local memory bounded (D1 tolerates the partial state — a dropped relay
/// frame is recoverable). Bootstrap relay mail (received before the first
/// command, which cannot happen in practice since no relays are open yet, but is
/// handled soundly anyway) is staged here and replayed after kernel
/// construction.
#[cfg(feature = "native")]
pub(super) struct MailScheduler {
    relay_backlog: VecDeque<PoolEvent>,
    /// Count of relay events dropped because the backlog was at
    /// [`RELAY_BACKLOG_CAP`] when a new event was stashed. Observable so the
    /// (recoverable) loss under flood is not silent.
    relay_backlog_drops: u64,
}

#[cfg(feature = "native")]
impl MailScheduler {
    pub(super) fn new() -> Self {
        Self {
            relay_backlog: VecDeque::new(),
            relay_backlog_drops: 0,
        }
    }

    /// Stash a relay event that must be processed but cannot run yet (the
    /// bootstrap pre-kernel replay path, or relay mail seen while draining the
    /// command lane).
    ///
    /// The backlog is capped at [`RELAY_BACKLOG_CAP`]: when it is full the
    /// oldest staged event is dropped (`pop_front` before `push_back`) and the
    /// drop counter is bumped, so a sustained flood bounds memory instead of
    /// growing without limit.
    pub(super) fn stash_relay(&mut self, event: PoolEvent) {
        if self.relay_backlog.len() >= RELAY_BACKLOG_CAP {
            // Drop the oldest staged event to make room (D1: partial state is
            // tolerated; the newest events are the most relevant to keep).
            self.relay_backlog.pop_front();
            self.relay_backlog_drops = self.relay_backlog_drops.saturating_add(1);
        }
        self.relay_backlog.push_back(event);
    }

    /// `true` while the backlog is at its [`RELAY_BACKLOG_CAP`] — the actor's
    /// command-drain loop uses this to STOP stashing and instead leave relay
    /// mail in the inbox channel, applying real backpressure to the pool rather
    /// than silently dropping under flood.
    pub(super) fn relay_backlog_is_full(&self) -> bool {
        self.relay_backlog.len() >= RELAY_BACKLOG_CAP
    }

    /// Number of relay events dropped on backlog overflow since construction.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn relay_backlog_drops(&self) -> u64 {
        self.relay_backlog_drops
    }

    /// Current backlog occupancy. Test-only observability of the bound.
    #[cfg(test)]
    pub(super) fn relay_backlog_len(&self) -> usize {
        self.relay_backlog.len()
    }

    /// Drain the priority command lane. Returns the [`CommandDrain`] budget
    /// state plus each command to dispatch through `for_each_command`.
    ///
    /// Returns `Err(())` to signal the inbox is closed (actor shutdown).
    ///
    /// This is the executable *specification* of the command-priority +
    /// fairness contract. The production loop in `mod.rs` implements the same
    /// contract inline (the per-command dispatch borrows `&mut kernel`,
    /// `&mut identity`, … and must early-return on `Shutdown`, which a closure
    /// boundary cannot express without threading that state out of the hot
    /// path); the inbox lane tests drive this method to lock the contract.
    #[cfg(test)]
    pub(super) fn drain_command_lane(
        &mut self,
        inbox: &Inbox,
        mut for_each_command: impl FnMut(ActorCommand),
    ) -> Result<CommandDrain, ()> {
        let mut drain = CommandDrain::new(COMMAND_DRAIN_BUDGET);
        loop {
            if !drain.can_drain_command() {
                break;
            }
            match inbox.try_recv() {
                Ok(ActorMail::Command(cmd)) => {
                    drain.record_command();
                    for_each_command(cmd);
                }
                Ok(ActorMail::Relay(event)) => {
                    // Relay mail does not consume the command budget; stash it
                    // for the relay lane below.
                    self.relay_backlog.push_back(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err(()),
            }
        }
        Ok(drain)
    }

    /// Drain a *bounded batch* of staged backlog events — up to
    /// [`RELAY_BACKLOG_DRAIN_BATCH`] — to process this iteration before the
    /// single blocking wait.
    ///
    /// Serving a batch (rather than one event per loop iteration) lets the
    /// backlog drain faster than a sustained flood fills it; capping the batch
    /// and then ALWAYS calling [`next_after_drain`](MailScheduler::next_after_drain)
    /// — which performs the one blocking `recv_timeout` — guarantees the actor
    /// reaches its single wait every iteration (D8) and cannot busy-spin while
    /// the backlog is non-empty.
    pub(super) fn drain_backlog_batch(&mut self) -> Vec<PoolEvent> {
        let take = self.relay_backlog.len().min(RELAY_BACKLOG_DRAIN_BATCH);
        self.relay_backlog.drain(..take).collect()
    }

    /// The post-batch step: the single blocking `recv_timeout(wait)` — the
    /// loop's *only* wait per iteration (D8).
    ///
    /// Backlog events are served by [`drain_backlog_batch`](MailScheduler::drain_backlog_batch)
    /// *before* this is called; this method no longer pops from the backlog, so
    /// a non-empty backlog never bypasses the blocking wait (kills the
    /// busy-spin). Any leftover backlog (beyond the batch) is served on the next
    /// iteration after this wait returns.
    ///
    /// `wait` is `Duration::ZERO` when more backlog work remains (the caller
    /// passes a zero wait so a full backlog keeps draining promptly without ever
    /// skipping the `recv_timeout` call), otherwise the computed compute-wait.
    pub(super) fn next_after_drain(&mut self, inbox: &Inbox, wait: std::time::Duration) -> LoopStep {
        match inbox.recv_timeout(wait) {
            Ok(ActorMail::Command(cmd)) => LoopStep::Command(cmd),
            Ok(ActorMail::Relay(event)) => LoopStep::Relay(event),
            Err(RecvTimeoutError::Timeout) => LoopStep::Idle,
            Err(RecvTimeoutError::Disconnected) => LoopStep::Shutdown,
        }
    }

    /// `true` while staged backlog events remain — the caller uses this to pass
    /// a `Duration::ZERO` wait to [`next_after_drain`](MailScheduler::next_after_drain)
    /// so a deep backlog keeps draining promptly while still hitting the
    /// blocking `recv_timeout` every iteration (D8).
    pub(super) fn has_backlog(&self) -> bool {
        !self.relay_backlog.is_empty()
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::thread;
    use std::time::{Duration, Instant};

    fn pool_event() -> PoolEvent {
        // A `Health` event is the cheapest `PoolEvent` to construct for lane
        // routing tests — its payload is not inspected here.
        PoolEvent::Health {
            h: nmp_network::pool::RelayHandle::for_test(0, 1),
            snapshot: nmp_network::pool::RelayHealth::default(),
        }
    }

    /// ADR-0050 §D3a core property: a thread blocked in `recv_timeout` with a
    /// long timeout wakes *immediately* when a command is sent — it does not
    /// wait out the timeout. This is the regression the whole change fixes.
    #[test]
    fn command_send_wakes_a_blocked_inbox() {
        let (tx, rx) = channel::<ActorMail>();
        let sender = CommandSender::new(tx);
        let inbox = Inbox::new(rx);

        let waiter = thread::spawn(move || {
            let start = Instant::now();
            // A 10s timeout: if the send does not wake us, this blocks the
            // full 10s and the assertion below fails the elapsed bound.
            let step = inbox.recv_timeout(Duration::from_secs(10));
            (start.elapsed(), step)
        });

        // Give the waiter a beat to reach the blocking recv, then send.
        thread::sleep(Duration::from_millis(50));
        sender
            .send(ActorCommand::Shutdown)
            .expect("inbox still open");

        let (elapsed, step) = waiter.join().expect("waiter thread");
        assert!(
            matches!(step, Ok(ActorMail::Command(ActorCommand::Shutdown))),
            "expected the sent command to be received"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "command send must wake the blocked inbox promptly, not wait the \
             10s timeout (elapsed: {elapsed:?})"
        );
    }

    /// Priority: when commands and relay mail are interleaved in the channel,
    /// the command lane is fully served first (up to budget) before any relay
    /// event is handed out.
    #[test]
    fn commands_are_served_before_relay_mail() {
        let (tx, rx) = channel::<ActorMail>();
        let inbox = Inbox::new(rx);
        let mut scheduler = MailScheduler::new();

        // Interleave: relay, command, relay, command. Keep `tx` alive so the
        // drain sees `Empty` (not `Disconnected`) once the queue is consumed.
        tx.send(ActorMail::Relay(pool_event())).unwrap();
        tx.send(ActorMail::Command(ActorCommand::Shutdown)).unwrap();
        tx.send(ActorMail::Relay(pool_event())).unwrap();
        tx.send(ActorMail::Command(ActorCommand::Shutdown)).unwrap();

        let mut commands = 0usize;
        let drain = scheduler
            .drain_command_lane(&inbox, |_cmd| commands += 1)
            .expect("inbox open during drain");

        assert_eq!(commands, 2, "both commands drained on the priority lane");
        assert!(!drain.hit_budget());
        // Only now do relay events surface — they come from the backlog (the
        // relay mail stashed while draining commands), served as a bounded
        // batch (zero wait, relay not starved).
        let batch = scheduler.drain_backlog_batch();
        assert_eq!(batch.len(), 2, "both stashed relay events drain in the batch");
        assert!(!scheduler.has_backlog(), "backlog fully drained");
        // With the backlog empty, the post-batch step is the blocking wait,
        // which times out to Idle here (channel empty, tx still alive).
        assert!(matches!(
            scheduler.next_after_drain(&inbox, Duration::from_millis(1)),
            LoopStep::Idle
        ));
    }

    /// Fairness: a sustained command burst yields to relay work at the budget.
    /// Commands beyond the budget stay in the channel; relay mail seen during
    /// the drain is served right after, never starved.
    #[test]
    fn command_burst_yields_to_relay_at_budget() {
        let (tx, rx) = channel::<ActorMail>();
        let inbox = Inbox::new(rx);
        let mut scheduler = MailScheduler::new();

        // One relay event, then a command flood larger than the budget.
        tx.send(ActorMail::Relay(pool_event())).unwrap();
        for _ in 0..(COMMAND_DRAIN_BUDGET + 10) {
            tx.send(ActorMail::Command(ActorCommand::Shutdown)).unwrap();
        }

        let mut commands = 0usize;
        let drain = scheduler
            .drain_command_lane(&inbox, |_cmd| commands += 1)
            .expect("inbox open");

        assert_eq!(
            commands, COMMAND_DRAIN_BUDGET,
            "the command lane stops exactly at the budget"
        );
        assert!(drain.hit_budget(), "budget reached → relay_wait is ZERO");
        // The relay event seen before the budget was hit is served immediately
        // (from the backlog batch), proving relay is not starved by the command
        // flood.
        let batch = scheduler.drain_backlog_batch();
        assert_eq!(batch.len(), 1, "the one stashed relay event drains");
        // Leftover commands remain in the channel for the next iteration (tx
        // kept alive — the live actor holds the relay sink, so the inbox does
        // not disconnect while draining).
        let mut leftover = 0usize;
        scheduler
            .drain_command_lane(&inbox, |_cmd| leftover += 1)
            .expect("inbox open");
        assert_eq!(leftover, 10, "commands beyond the budget were not dropped");
        drop(tx);
    }

    /// A timeout (no mail) yields `Idle`; a closed inbox yields `Shutdown`.
    #[test]
    fn timeout_is_idle_and_closed_inbox_is_shutdown() {
        let (tx, rx) = channel::<ActorMail>();
        let inbox = Inbox::new(rx);
        let mut scheduler = MailScheduler::new();

        assert!(matches!(
            scheduler.next_after_drain(&inbox, Duration::from_millis(1)),
            LoopStep::Idle
        ));

        drop(tx);
        assert!(matches!(
            scheduler.next_after_drain(&inbox, Duration::from_millis(1)),
            LoopStep::Shutdown
        ));
    }

    /// #1264 load test — a SUSTAINED relay flood keeps the backlog BOUNDED and
    /// the loop ALWAYS reaches its single blocking wait (no busy-spin).
    ///
    /// This drives the exact lane loop the production actor runs: each
    /// "iteration" drains the command lane (stashing relay mail, but stopping
    /// once the backlog is full), serves a bounded backlog batch, then performs
    /// the one `recv_timeout`. We flood far more relay events than
    /// `RELAY_BACKLOG_CAP` and assert:
    ///   1. `relay_backlog.len()` never exceeds `RELAY_BACKLOG_CAP` (memory is
    ///      bounded — the pre-fix unbounded `VecDeque` would grow without limit);
    ///   2. the loop reaches `recv_timeout` every iteration (the pre-fix
    ///      one-pop-per-call path returned `Relay` while the backlog was
    ///      non-empty and NEVER blocked → busy-spin);
    ///   3. on overflow the drop counter is bumped (loss is observable, D1).
    ///
    /// Without the cap+batch fix this test fails on (1) (backlog grows past the
    /// cap) and (2) (`recv_timeout` is never reached while backlog is
    /// non-empty).
    #[test]
    fn sustained_relay_flood_stays_bounded_and_reaches_the_blocking_wait() {
        let (tx, rx) = channel::<ActorMail>();
        let inbox = Inbox::new(rx);
        let mut scheduler = MailScheduler::new();

        // Flood: an order of magnitude more relay events than the cap, all
        // queued before we start draining (simulating a relay replaying
        // thousands of historical events faster than the actor drains them).
        const FLOOD: usize = RELAY_BACKLOG_CAP * 10;
        for _ in 0..FLOOD {
            tx.send(ActorMail::Relay(pool_event())).unwrap();
        }

        let mut recv_timeout_reached = 0usize;
        let mut processed = 0usize;
        // Bound the loop generously; it must terminate well before this.
        for _iteration in 0..(FLOOD * 2) {
            // 1. Command lane: drain relay mail from the channel into the
            //    backlog. `stash_relay` itself enforces the cap (dropping the
            //    OLDEST on overflow) — this models the safety-net / bootstrap
            //    path where stashing is not gated on `relay_backlog_is_full`,
            //    proving the cap bounds memory even under unconditional stash.
            loop {
                match inbox.try_recv() {
                    Ok(ActorMail::Relay(event)) => scheduler.stash_relay(event),
                    Ok(ActorMail::Command(_)) => {}
                    Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                }
            }

            // INVARIANT 1: the backlog is bounded at every point.
            assert!(
                scheduler.relay_backlog_len() <= RELAY_BACKLOG_CAP,
                "relay backlog must stay bounded by RELAY_BACKLOG_CAP={}, was {}",
                RELAY_BACKLOG_CAP,
                scheduler.relay_backlog_len()
            );

            // 2. Serve a bounded batch.
            let batch = scheduler.drain_backlog_batch();
            assert!(
                batch.len() <= RELAY_BACKLOG_DRAIN_BATCH,
                "batch must be bounded by RELAY_BACKLOG_DRAIN_BATCH"
            );
            processed += batch.len();

            // 3. ALWAYS perform the single blocking wait (D8). When backlog
            //    work remains we pass ZERO so it returns immediately, but the
            //    call is still made — that is what kills the busy-spin.
            let wait = if scheduler.has_backlog() {
                Duration::ZERO
            } else {
                Duration::from_millis(1)
            };
            match scheduler.next_after_drain(&inbox, wait) {
                LoopStep::Relay(_) => processed += 1,
                LoopStep::Idle => {
                    recv_timeout_reached += 1;
                    // Channel drained and backlog empty → flood fully handled.
                    if !scheduler.has_backlog() {
                        break;
                    }
                }
                LoopStep::Command(_) => {}
                LoopStep::Shutdown => break,
            }
        }

        // The actor reached its blocking `recv_timeout` (returning Idle on the
        // empty channel) — the loop did NOT busy-spin forever bypassing the
        // wait while the backlog was non-empty.
        assert!(
            recv_timeout_reached >= 1,
            "the loop must reach its single blocking recv_timeout, not busy-spin"
        );

        // Overflow drops were counted: FLOOD far exceeds what we drained before
        // overflow, so at least some events were dropped, and the loss is
        // observable rather than silent.
        assert!(
            scheduler.relay_backlog_drops() > 0,
            "backlog overflow must bump the observable drop counter"
        );
        // Bounded memory means we cannot have *processed* all FLOOD events —
        // the dropped ones are gone (D1: recoverable via re-subscription).
        assert!(
            processed < FLOOD,
            "a bounded backlog drops the oldest under flood (processed {processed} of {FLOOD})"
        );
        drop(tx);
    }

    /// `CommandSender::send` on a closed inbox returns the undelivered command
    /// (mpsc-`SendError` parity) rather than losing it.
    #[test]
    fn closed_inbox_send_returns_the_command() {
        let (tx, rx) = channel::<ActorMail>();
        let sender = CommandSender::new(tx);
        drop(rx);
        let err = sender
            .send(ActorCommand::Shutdown)
            .expect_err("send on closed inbox must error");
        assert!(matches!(err.0, ActorCommand::Shutdown));
    }
}
