//! Command-side handle for the single waking actor inbox.
//!
//! Split from `inbox.rs` so the scheduler and sender each stay under the
//! repository file-size hard cap while preserving the same actor inbox seam.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SendError, Sender, SyncSender, TrySendError};
use std::sync::Arc;

use super::ActorCommand;
#[cfg(feature = "native")]
use super::LifecycleCommand;
use crate::actor::inbox::ActorMail;

/// Hard cap on the unified actor inbox.
///
/// The FFI/native dispatch path is fire-and-forget, so callers must never block
/// waiting for actor capacity. A bounded inbox plus `try_send` gives the actor a
/// fixed memory ceiling under command floods. When full, newly-arriving commands
/// are shed and counted; commands already accepted keep their FIFO order.
pub const ACTOR_INBOX_CAPACITY: usize = 4096;

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

/// Result of a nonblocking command send.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandSendStatus {
    /// The command was accepted by the actor inbox.
    Enqueued,
    /// The bounded inbox was full, so the command was dropped and counted.
    DroppedFull,
}

#[derive(Clone, Debug)]
pub(super) enum ActorInboxTx {
    Unbounded(Sender<ActorMail>),
    Bounded(SyncSender<ActorMail>),
}

impl ActorInboxTx {
    pub(super) fn send_mail(&self, mail: ActorMail) -> Result<(), TrySendError<ActorMail>> {
        match self {
            ActorInboxTx::Unbounded(tx) => tx
                .send(mail)
                .map_err(|SendError(mail)| TrySendError::Disconnected(mail)),
            ActorInboxTx::Bounded(tx) => tx.try_send(mail),
        }
    }
}

/// A cheap, cloneable handle for sending [`ActorCommand`]s into the actor
/// inbox.
///
/// This is the single command-send seam (ADR-0072 §D3a). It replaces the bare
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
    tx: ActorInboxTx,
    command_drops: Arc<AtomicU64>,
}

impl CommandSender {
    /// Construct the default bounded actor inbox and its command handle.
    #[must_use]
    pub fn bounded_channel() -> (Self, Receiver<ActorMail>) {
        Self::bounded_channel_with_capacity(ACTOR_INBOX_CAPACITY)
    }

    /// Construct a bounded actor inbox with an explicit capacity.
    #[must_use]
    pub fn bounded_channel_with_capacity(capacity: usize) -> (Self, Receiver<ActorMail>) {
        let (tx, rx) = std::sync::mpsc::sync_channel(capacity);
        (Self::new_bounded(tx), rx)
    }

    /// Wrap an unbounded inbox sender. Production actor construction must use
    /// [`bounded_channel`](Self::bounded_channel); this constructor remains for
    /// focused tests and capture-only harnesses that do not model backpressure.
    #[must_use]
    pub fn new(tx: Sender<ActorMail>) -> Self {
        Self {
            tx: ActorInboxTx::Unbounded(tx),
            command_drops: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Wrap a bounded inbox sender.
    #[must_use]
    pub fn new_bounded(tx: SyncSender<ActorMail>) -> Self {
        Self {
            tx: ActorInboxTx::Bounded(tx),
            command_drops: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Number of command sends shed because the bounded inbox was full.
    #[must_use]
    pub fn command_drops(&self) -> u64 {
        self.command_drops.load(Ordering::Relaxed)
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
    pub fn send(&self, command: ActorCommand) -> Result<CommandSendStatus, CommandSendError> {
        // `send` only ever enqueues `Command` mail, so the error payload (when
        // the inbox is closed) is exactly the command we just tried to send.
        // We recover it with `if let` (never `unreachable!`, D6) and fall back
        // to a no-payload-loss `Shutdown` only on the structurally-impossible
        // relay arm rather than panicking.
        self.tx
            .send_mail(ActorMail::Command(command))
            .map(|()| CommandSendStatus::Enqueued)
            .or_else(|err| match err {
                TrySendError::Full(_mail) => {
                    self.command_drops.fetch_add(1, Ordering::Relaxed);
                    Ok(CommandSendStatus::DroppedFull)
                }
                TrySendError::Disconnected(mail) => Err(mail),
            })
            .map_err(|mail| match mail {
                ActorMail::Command(cmd) => CommandSendError(cmd),
                #[cfg(feature = "native")]
                ActorMail::Relay(_) => {
                    CommandSendError(ActorCommand::Lifecycle(LifecycleCommand::Shutdown))
                }
            })
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
    tx: ActorInboxTx,
}

#[cfg(feature = "native")]
impl RelayMailSink {
    fn new(tx: ActorInboxTx) -> Self {
        Self { tx }
    }
}

#[cfg(feature = "native")]
impl nmp_network::pool::PoolEventSink for RelayMailSink {
    fn send_event(&self, event: nmp_network::pool::PoolEvent) {
        let _ = self.tx.send_mail(ActorMail::Relay(event));
    }
}

#[path = "inbox/typed_sender.rs"]
mod typed_sender;
