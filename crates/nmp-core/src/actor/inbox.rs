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
            // #1264: once the relay backlog is at RELAY_BACKLOG_CAP, STOP
            // draining the inbox channel. Pulling more relay mail out only to
            // drop the oldest staged event (or this one) does no useful work and
            // defeats backpressure — leaving relay mail in the bounded mpsc
            // channel lets pressure build there (and ultimately at the pool's
            // translator), the correct place to absorb a flood. The next
            // iteration's bounded batch drain frees room. Commands already ahead
            // in the channel are still served by the prior `first_command`
            // replay / earlier `try_recv`s, so commands are not starved by a
            // relay backlog — we only stop pulling *new* mail forward.
            if self.relay_backlog_is_full() {
                break;
            }
            match inbox.try_recv() {
                Ok(ActorMail::Command(cmd)) => {
                    drain.record_command();
                    commands.push(cmd);
                }
                Ok(ActorMail::Relay(event)) => {
                    // Relay mail does not consume the command budget; stash it
                    // for the relay lane below. `stash_relay` honors the
                    // RELAY_BACKLOG_CAP bound (drops oldest + bumps the drop
                    // counter on overflow) so a flood bounds memory.
                    self.stash_relay(event);
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
