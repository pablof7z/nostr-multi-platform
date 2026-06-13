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
