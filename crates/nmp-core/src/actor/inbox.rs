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

#[cfg(feature = "native")]
use super::fairness::{CommandDrain, COMMAND_DRAIN_BUDGET};
use super::ActorCommand;
#[cfg(feature = "native")]
use nmp_network::pool::PoolEvent;

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
/// 2. [`next_after_drain`](MailScheduler::next_after_drain) — serve one relay
///    event: a stashed backlog item if present (zero wait, relay not starved),
///    otherwise the single blocking `recv_timeout` that is the loop's *only*
///    wait (D8). A command that arrives during the wait is returned for the
///    command path, preserving "command sends wake the actor".
///
/// The backlog never grows beyond what the channel already held *before* the
/// command budget was hit, so there is no unbounded local buffering (ADR-0050
/// §D3a constraint). Bootstrap relay mail (received before the first command,
/// which cannot happen in practice since no relays are open yet, but is handled
/// soundly anyway) is staged here via [`stash_relay`](MailScheduler::stash_relay)
/// and replayed after kernel construction.
#[cfg(feature = "native")]
pub(super) struct MailScheduler {
    relay_backlog: VecDeque<PoolEvent>,
}

/// Result of one [`MailScheduler::drain_command_lane`] pass: the commands to
/// dispatch (in arrival order, `first_command` first), the budget state used to
/// compute the post-drain relay wait, and whether the inbox is now closed.
#[cfg(feature = "native")]
pub(super) struct CommandLaneDrain {
    /// Commands drained this iteration, to be dispatched by the caller with its
    /// `&mut kernel` / `&mut identity` borrows.
    pub(super) commands: Vec<ActorCommand>,
    /// Budget accounting — `relay_wait`/`hit_budget` drive the relay lane wait.
    pub(super) drain: CommandDrain,
    /// True when every `CommandSender` clone has dropped (actor teardown).
    pub(super) disconnected: bool,
}

#[cfg(feature = "native")]
impl MailScheduler {
    pub(super) fn new() -> Self {
        Self {
            relay_backlog: VecDeque::new(),
        }
    }

    /// Stash a relay event that must be processed but cannot run yet (the
    /// bootstrap pre-kernel replay path, or relay mail seen while draining the
    /// command lane).
    pub(super) fn stash_relay(&mut self, event: PoolEvent) {
        self.relay_backlog.push_back(event);
    }

    /// Drain the priority command lane. Replays a `first_command` (a command
    /// dequeued by the previous iteration's blocking `recv_timeout` and held
    /// for priority service) ahead of the channel, then non-blockingly drains
    /// queued commands up to [`COMMAND_DRAIN_BUDGET`], stashing any relay mail
    /// seen along the way into the backlog so it is served after the command
    /// lane (never starved). Returns the drained commands *and* the
    /// [`CommandDrain`] budget state so the caller can dispatch each command
    /// with its `&mut kernel` / `&mut identity` borrows and compute the
    /// post-drain relay wait.
    ///
    /// This is the single, non-duplicated drain: the production `run_actor`
    /// loop routes through it (issue #1231 follow-up #3 — previously it
    /// reimplemented the same budget/priority/backlog logic inline, which could
    /// drift from this "executable specification" silently). Returning the
    /// commands as a `Vec` rather than invoking a `FnMut` is what lets the
    /// production side keep the per-command `&mut`-heavy dispatch (and its
    /// early-return on `Shutdown`) outside the closure boundary that previously
    /// blocked this unification.
    pub(super) fn drain_command_lane(
        &mut self,
        inbox: &Inbox,
        first_command: Option<ActorCommand>,
    ) -> CommandLaneDrain {
        let mut drain = CommandDrain::new(COMMAND_DRAIN_BUDGET);
        let mut commands = Vec::new();
        let mut disconnected = false;

        if let Some(cmd) = first_command {
            drain.record_command();
            commands.push(cmd);
        }

        loop {
            if !drain.can_drain_command() {
                break;
            }
            match inbox.try_recv() {
                Ok(ActorMail::Command(cmd)) => {
                    drain.record_command();
                    commands.push(cmd);
                }
                Ok(ActorMail::Relay(event)) => {
                    // Relay mail does not consume the command budget; stash it
                    // for the relay lane below.
                    self.relay_backlog.push_back(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        CommandLaneDrain {
            commands,
            drain,
            disconnected,
        }
    }

    /// Decide the post-drain step: a backlog relay event (zero wait), else the
    /// single blocking `recv_timeout(wait)`.
    pub(super) fn next_after_drain(&mut self, inbox: &Inbox, wait: std::time::Duration) -> LoopStep {
        if let Some(event) = self.relay_backlog.pop_front() {
            return LoopStep::Relay(event);
        }
        match inbox.recv_timeout(wait) {
            Ok(ActorMail::Command(cmd)) => LoopStep::Command(cmd),
            Ok(ActorMail::Relay(event)) => LoopStep::Relay(event),
            Err(RecvTimeoutError::Timeout) => LoopStep::Idle,
            Err(RecvTimeoutError::Disconnected) => LoopStep::Shutdown,
        }
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

        let result = scheduler.drain_command_lane(&inbox, None);
        assert!(!result.disconnected, "inbox open during drain");

        assert_eq!(
            result.commands.len(),
            2,
            "both commands drained on the priority lane"
        );
        assert!(!result.drain.hit_budget());
        // Only now does a relay event surface — and it comes from the backlog
        // (the relay mail stashed while draining commands), zero wait.
        assert!(matches!(
            scheduler.next_after_drain(&inbox, Duration::ZERO),
            LoopStep::Relay(_)
        ));
        assert!(matches!(
            scheduler.next_after_drain(&inbox, Duration::ZERO),
            LoopStep::Relay(_)
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

        let result = scheduler.drain_command_lane(&inbox, None);
        assert!(!result.disconnected, "inbox open");

        assert_eq!(
            result.commands.len(),
            COMMAND_DRAIN_BUDGET,
            "the command lane stops exactly at the budget"
        );
        assert!(
            result.drain.hit_budget(),
            "budget reached → relay_wait is ZERO"
        );
        // The relay event seen before the budget was hit is served immediately
        // (from the backlog), proving relay is not starved by the command flood.
        assert!(matches!(
            scheduler.next_after_drain(&inbox, Duration::ZERO),
            LoopStep::Relay(_)
        ));
        // Leftover commands remain in the channel for the next iteration (tx
        // kept alive — the live actor holds the relay sink, so the inbox does
        // not disconnect while draining).
        let leftover = scheduler.drain_command_lane(&inbox, None);
        assert!(!leftover.disconnected, "inbox open");
        assert_eq!(
            leftover.commands.len(),
            10,
            "commands beyond the budget were not dropped"
        );
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
