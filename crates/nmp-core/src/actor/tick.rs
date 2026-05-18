//! Idle-tick timing helpers — `next_actor_msg`, `emit_now`, `flush_due`, and
//! the `emit_interval` utility.  Separated from the main loop so that the D8
//! invariant ("emit only when state changed") is concentrated in one file.

use crate::kernel::Kernel;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use super::ActorMsg;

pub(super) fn next_actor_msg(
    actor_rx: &Receiver<ActorMsg>,
    kernel: &Kernel,
    running: bool,
    last_emit: Instant,
    emit_hz: u32,
) -> Result<Option<ActorMsg>, ()> {
    if running && kernel.changed_since_emit() {
        let wait = emit_interval(emit_hz)
            .checked_sub(last_emit.elapsed())
            .unwrap_or(Duration::ZERO);
        if wait.is_zero() {
            return Ok(None);
        }
        return match actor_rx.recv_timeout(wait) {
            Ok(message) => Ok(Some(message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        };
    }

    if running {
        // Poll at 250 ms so time-based kernel gates (e.g. contacts_deadline)
        // are checked even when no relay messages arrive.
        return match actor_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(message) => Ok(Some(message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        };
    }

    actor_rx.recv().map(Some).map_err(|_| ())
}

pub(super) fn emit_interval(emit_hz: u32) -> Duration {
    Duration::from_secs_f64(1.0 / emit_hz.max(1) as f64)
}

pub(super) fn flush_due(kernel: &Kernel, running: bool, last_emit: Instant, emit_hz: u32) -> bool {
    running && kernel.changed_since_emit() && last_emit.elapsed() >= emit_interval(emit_hz)
}

pub(super) fn emit_now(
    kernel: &mut Kernel,
    running: bool,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
) {
    let update = kernel.make_update(running);
    let _ = update_tx.send(update);
    *last_emit = Instant::now();
}

// ── D8 regression test ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::actor::{run_actor, ActorCommand};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    /// Verifies that idle ticks do not emit snapshots when kernel state has not
    /// changed (D8: zero false-wakeup allocations after warmup — codex T23 P2).
    ///
    /// The actor is spawned WITHOUT sending `Start`, so no relays connect and
    /// `changed_since_emit` never becomes true.  Over 1 s the 250 ms idle-poll
    /// fires ~4 times; none should produce a snapshot.
    #[test]
    fn idle_ticks_do_not_emit_snapshots_when_state_unchanged() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
        let (upd_tx, upd_rx) = mpsc::channel::<String>();
        thread::spawn(move || run_actor(cmd_rx, upd_tx));

        // Wait long enough for several idle-poll cycles without any commands.
        thread::sleep(Duration::from_millis(1_000));

        let _ = cmd_tx.send(ActorCommand::Shutdown);

        let mut idle_count = 0_usize;
        while upd_rx.try_recv().is_ok() {
            idle_count += 1;
        }

        assert_eq!(
            idle_count, 0,
            "D8 regression: actor emitted {idle_count} snapshot(s) without any \
             Start command or state change; expected 0"
        );
    }
}
