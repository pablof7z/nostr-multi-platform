//! Inbox command/relay lane priority + fairness tests.
//!
//! Extracted from `inbox.rs` to keep that file under the 500 LOC hard cap
//! (AGENTS.md). These tests drive the `MailScheduler` drain contract — the
//! executable specification that the production `run_actor` loop in `mod.rs`
//! routes through (issue #1231 follow-up #3). They reach into the inbox's
//! `pub(super)` surface, so they live as a sibling module of `inbox` within
//! the `actor` module rather than inside `inbox` itself.

use super::fairness::COMMAND_DRAIN_BUDGET;
use super::inbox::{
    ActorMail, CommandSender, Inbox, LoopStep, MailScheduler,
};
use super::ActorCommand;
use nmp_network::pool::PoolEvent;
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
    // Only now do the relay events surface — both stashed into the backlog
    // while the command lane was drained (#1264 two-step relay drain: the
    // production loop serves them via `drain_backlog_batch` before the single
    // blocking wait, never from `next_after_drain`).
    let backlog = scheduler.drain_backlog_batch();
    assert_eq!(
        backlog.len(),
        2,
        "both interleaved relay events were stashed for the relay lane"
    );
    // With the backlog now empty the inbox channel is exhausted too: the next
    // step is `Idle` (open inbox, nothing queued), not another relay event.
    assert!(matches!(
        scheduler.next_after_drain(&inbox, Duration::ZERO),
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
    // The relay event seen before the budget was hit was stashed into the
    // backlog (served by the production loop's `drain_backlog_batch` right
    // after this drain), proving relay is not starved by the command flood.
    let backlog = scheduler.drain_backlog_batch();
    assert_eq!(
        backlog.len(),
        1,
        "the relay event seen during the command drain was stashed, not dropped"
    );
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
