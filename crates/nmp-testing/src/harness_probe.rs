//! Event-driven harness waits: Doctrine D8 (no polling) for test/perf rigs.
//!
//! Production code may never `sleep`-then-check; the test and perf harnesses
//! that *validate* that contract must not either. A [`FrameProbe`] replaces a
//! fixed-interval spin loop with a blocking channel: each harness event (a
//! captured FFI frame, a recorded action/revision) wakes the waiter, which
//! re-evaluates its predicate over the shared state it owns. [`recv_until`]
//! returns the instant the predicate holds, or `false` once the timeout is
//! reached, never on a wall-clock tick.
//!
//! The split mirrors the producer/consumer seam: a [`ProbeSignal`] is cloned
//! into the event source (e.g. an FFI update callback) and fired on every
//! event; the [`FrameProbe`] stays on the waiting thread. Signals carry no
//! payload: the source updates the shared state first, then notifies, so one
//! re-check after a burst is sufficient and a dropped/coalesced wake only costs
//! a redundant notification, never correctness.
//!
//! [`recv_until`]: FrameProbe::recv_until
//!
//! # Latest-frame channel helpers
//!
//! Some harness rigs carry their payload *on* the channel rather than in a
//! caller-owned state cell: each message is a fully-formed frame and only the
//! newest one matters (older snapshots are superseded). For those,
//! [`drain_latest`] and [`recv_latest_until`] block on the actual deadline
//! instead of a fixed-interval `recv_timeout` poll: a single blocking wait wakes
//! exactly when a frame arrives or the deadline passes, never on a wall-clock
//! tick, and any frames already queued behind the first are collapsed into the
//! newest.

use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::{Duration, Instant};

/// Drain every frame currently queued on `rx` without blocking and return the
/// newest, or `None` if the channel is empty (or disconnected with nothing
/// buffered). Used to collapse a burst of superseded frames into the one that
/// matters.
pub fn drain_latest<T>(rx: &Receiver<T>) -> Option<T> {
    let mut latest = None;
    while let Ok(frame) = rx.try_recv() {
        latest = Some(frame);
    }
    latest
}

/// Block until a frame arrives or `deadline` passes, then return the newest
/// queued frame.
///
/// The wait is a single `recv_timeout` to the real deadline, not a periodic
/// poll: it wakes the instant a frame is delivered. Once the first frame
/// arrives, any others already queued behind it are drained so the newest is
/// returned (older snapshots are superseded). Returns `None` only when the
/// deadline is reached with nothing queued, or the sender disconnected before a
/// frame arrived.
pub fn recv_latest_until<T>(rx: &Receiver<T>, deadline: Instant) -> Option<T> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .unwrap_or(Duration::ZERO);
    if remaining.is_zero() {
        // Deadline already reached: take whatever is queued without blocking.
        return drain_latest(rx);
    }
    let first = match rx.recv_timeout(remaining) {
        Ok(frame) => frame,
        Err(RecvTimeoutError::Timeout) => return drain_latest(rx),
        Err(RecvTimeoutError::Disconnected) => return None,
    };
    // A frame arrived; collapse any others already queued into the newest.
    Some(drain_latest(rx).unwrap_or(first))
}

/// The notifying half, wired into an event source. Cloneable so several
/// producers can wake the same [`FrameProbe`].
#[derive(Clone)]
pub struct ProbeSignal {
    tx: SyncSender<()>,
}

impl ProbeSignal {
    /// Wake any waiter blocked in [`FrameProbe::recv_until`]. A disconnected
    /// receiver (probe already dropped) is not an error: the source keeps
    /// running and the lost wake is harmless. The channel is one-slot bounded:
    /// a pending wake already represents "state changed", so repeated frame
    /// ticks coalesce instead of accumulating during long soak/perf phases.
    pub fn notify(&self) {
        match self.tx.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) | Err(TrySendError::Disconnected(())) => {}
        }
    }
}

/// The waiting half. Blocks on harness events and re-checks a caller-owned
/// predicate, never sleeping on a fixed interval.
pub struct FrameProbe {
    rx: Receiver<()>,
}

impl FrameProbe {
    /// Create a connected `(signal, probe)` pair backed by a blocking channel.
    pub fn new() -> (ProbeSignal, FrameProbe) {
        let (tx, rx) = sync_channel(1);
        (ProbeSignal { tx }, FrameProbe { rx })
    }

    /// Block until `ready()` returns `true` or `timeout` elapses, waking on
    /// every signalled harness event. Returns `true` if the predicate was
    /// satisfied, `false` on timeout.
    ///
    /// `ready` is evaluated once up front so an already-satisfied condition
    /// returns without waiting, and again after each wake. A burst of signals
    /// is drained before re-checking so one evaluation covers many frames. A
    /// disconnected source collapses to a final `ready()` check rather than an
    /// early `false`, so a producer that finishes mid-wait is still observed.
    pub fn recv_until(&self, timeout: Duration, mut ready: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        if ready() {
            return true;
        }
        loop {
            let now = Instant::now();
            if now >= deadline {
                return ready();
            }
            match self.rx.recv_timeout(deadline - now) {
                Ok(()) => {
                    // Coalesce a burst of wakes into a single re-check.
                    while self.rx.try_recv().is_ok() {}
                    if ready() {
                        return true;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return ready(),
                Err(RecvTimeoutError::Disconnected) => return ready(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn returns_true_immediately_when_already_ready() {
        let (_signal, probe) = FrameProbe::new();
        // No signal is ever sent; the up-front check must short-circuit.
        assert!(probe.recv_until(Duration::from_secs(5), || true));
    }

    #[test]
    fn wakes_on_signal_before_timeout() {
        let (signal, probe) = FrameProbe::new();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_w = Arc::clone(&flag);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            flag_w.store(true, Ordering::SeqCst);
            signal.notify();
        });
        // A generous timeout the test must NOT consume: it returns as soon as
        // the signal wakes it and the predicate flips.
        assert!(probe.recv_until(Duration::from_secs(5), || flag.load(Ordering::SeqCst)));
    }

    #[test]
    fn returns_false_on_timeout_when_never_ready() {
        let (_signal, probe) = FrameProbe::new();
        assert!(!probe.recv_until(Duration::from_millis(20), || false));
    }

    #[test]
    fn coalesces_repeated_signals_without_blocking() {
        let (signal, probe) = FrameProbe::new();
        signal.notify();
        signal.notify();
        signal.notify();
        assert!(probe.recv_until(Duration::from_secs(5), || true));
    }

    #[test]
    fn observes_final_state_after_source_disconnects() {
        let (signal, probe) = FrameProbe::new();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_w = Arc::clone(&flag);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            flag_w.store(true, Ordering::SeqCst);
            signal.notify();
            // Drop `signal` here so the channel disconnects; the final `ready()`
            // check must still see the stored state.
        });
        assert!(probe.recv_until(Duration::from_secs(5), || flag.load(Ordering::SeqCst)));
    }

    #[test]
    fn drain_latest_returns_newest_queued_frame() {
        let (tx, rx) = std::sync::mpsc::channel::<u32>();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();
        assert_eq!(drain_latest(&rx), Some(3));
    }

    #[test]
    fn drain_latest_returns_none_when_empty() {
        let (_tx, rx) = std::sync::mpsc::channel::<u32>();
        assert_eq!(drain_latest(&rx), None);
    }

    #[test]
    fn recv_latest_until_wakes_on_delayed_frame() {
        let (tx, rx) = std::sync::mpsc::channel::<u32>();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            tx.send(7).unwrap();
        });
        // A generous deadline the call must NOT consume: it returns as soon as
        // the frame is delivered.
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(recv_latest_until(&rx, deadline), Some(7));
    }

    #[test]
    fn recv_latest_until_collapses_burst_to_newest() {
        let (tx, rx) = std::sync::mpsc::channel::<u32>();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(recv_latest_until(&rx, deadline), Some(3));
    }

    #[test]
    fn recv_latest_until_returns_none_on_deadline_with_no_frame() {
        let (_tx, rx) = std::sync::mpsc::channel::<u32>();
        let deadline = Instant::now() + Duration::from_millis(20);
        assert_eq!(recv_latest_until(&rx, deadline), None);
    }

    #[test]
    fn recv_latest_until_returns_none_when_sender_disconnected() {
        let (tx, rx) = std::sync::mpsc::channel::<u32>();
        drop(tx);
        let deadline = Instant::now() + Duration::from_secs(5);
        // Disconnected with nothing buffered returns immediately, not at the
        // deadline.
        assert_eq!(recv_latest_until(&rx, deadline), None);
    }
}
