//! #2767 — the backpressure drop counters (`command_drops`,
//! `relay_backlog_drops`) round-trip through the kernel snapshot accessors,
//! mirroring [`queue_depth_tests`](super::queue_depth_tests) exactly. Unlike
//! `actor_queue_depth` (a `u32`-saturating gauge) these are monotonic `u64`
//! counters — no saturation.

use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::atomic::{AtomicU64, Ordering};

/// The `Arc<AtomicU64>` shared with the host's `CommandSender` must reach
/// `command_drops()` so the snapshot surfaces real shed-command counts, and
/// must survive `Reset`.
#[test]
fn command_drops_handle_surfaces_on_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert_eq!(
        kernel.command_drops(),
        0,
        "unbound kernel reports zero command drops"
    );

    let handle = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_command_drops_handle(std::sync::Arc::clone(&handle));

    // External mutation (mirrors `CommandSender::send` incrementing on a full inbox).
    handle.fetch_add(3, Ordering::Relaxed);
    assert_eq!(
        kernel.command_drops(),
        3,
        "kernel must observe external Arc increments"
    );

    // No saturation: this is a monotonic counter, not a u32 gauge.
    handle.store(u64::from(u32::MAX) + 100, Ordering::Relaxed);
    assert_eq!(
        kernel.command_drops(),
        u64::from(u32::MAX) + 100,
        "command_drops must not saturate at u32::MAX"
    );
    handle.store(3, Ordering::Relaxed);

    // Reset round-trip: extract → reinstall onto fresh kernel.
    let extracted = kernel.take_command_drops_handle_for_reset();
    assert!(extracted.is_some());
    let mut fresh = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    fresh.set_command_drops_handle(extracted.unwrap());
    handle.fetch_add(1, Ordering::Relaxed);
    assert_eq!(
        fresh.command_drops(),
        4,
        "Reset must preserve the command-drop counter via take→set round-trip"
    );
}

/// The `Arc<AtomicU64>` shared with the actor's `MailScheduler` must reach
/// `relay_backlog_drops()` so the snapshot surfaces real shed-relay-event
/// counts, and must survive `Reset`.
#[test]
fn relay_backlog_drops_handle_surfaces_on_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert_eq!(
        kernel.relay_backlog_drops(),
        0,
        "unbound kernel reports zero relay-backlog drops"
    );

    let handle = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_relay_backlog_drops_handle(std::sync::Arc::clone(&handle));

    // External mutation (mirrors `MailScheduler::stash_relay` incrementing on backlog overflow).
    handle.fetch_add(5, Ordering::Relaxed);
    assert_eq!(
        kernel.relay_backlog_drops(),
        5,
        "kernel must observe external Arc increments"
    );

    // Reset round-trip: extract → reinstall onto fresh kernel.
    let extracted = kernel.take_relay_backlog_drops_handle_for_reset();
    assert!(extracted.is_some());
    let mut fresh = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    fresh.set_relay_backlog_drops_handle(extracted.unwrap());
    handle.fetch_add(2, Ordering::Relaxed);
    assert_eq!(
        fresh.relay_backlog_drops(),
        7,
        "Reset must preserve the relay-backlog-drop counter via take→set round-trip"
    );
}

/// `external_event_sink_channel_overflow_drops()` surfaces real dispatcher
/// channel-overflow counts in the kernel snapshot, not just the dispatcher's
/// test-only diagnostics accessor.
#[test]
fn external_event_sink_channel_overflow_drops_handle_surfaces_on_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert_eq!(
        kernel.external_event_sink_channel_overflow_drops(),
        0,
        "unbound external-sink drop counter defaults to zero"
    );

    let handle = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_external_event_sink_channel_overflow_drops_handle(std::sync::Arc::clone(&handle));
    handle.fetch_add(4, Ordering::Relaxed);
    assert_eq!(
        kernel.external_event_sink_channel_overflow_drops(),
        4,
        "bound external-sink channel overflow drops must be visible"
    );

    let extracted = kernel.take_external_event_sink_channel_overflow_drops_handle_for_reset();
    let mut fresh = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    fresh.set_external_event_sink_channel_overflow_drops_handle(extracted.unwrap());
    assert_eq!(
        fresh.external_event_sink_channel_overflow_drops(),
        4,
        "same external-sink drop handle must preserve the monotonic count across reset"
    );
}

/// Load-bearing observability assertion (#2767): a forced drop on each
/// counter must be visible in the serialized kernel snapshot, not just via
/// the raw accessor — this is what makes the drop "host-visible" rather than
/// silent.
#[test]
fn drop_counters_observable_in_snapshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let command_drops = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_command_drops_handle(std::sync::Arc::clone(&command_drops));
    let relay_backlog_drops = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_relay_backlog_drops_handle(std::sync::Arc::clone(&relay_backlog_drops));
    let external_event_sink_channel_overflow_drops = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_external_event_sink_channel_overflow_drops_handle(std::sync::Arc::clone(
        &external_event_sink_channel_overflow_drops,
    ));

    // Force a drop on each counter, exactly as the production paths would.
    command_drops.fetch_add(2, Ordering::Relaxed);
    relay_backlog_drops.fetch_add(9, Ordering::Relaxed);
    external_event_sink_channel_overflow_drops.fetch_add(4, Ordering::Relaxed);

    let json = kernel.make_update_value_for_test(true);
    assert_eq!(
        json["metrics"]["command_drops"], 2,
        "forced command drop must be observable in the snapshot"
    );
    assert_eq!(
        json["metrics"]["relay_backlog_drops"], 9,
        "forced relay-backlog drop must be observable in the snapshot"
    );
    assert_eq!(
        json["metrics"]["external_event_sink_channel_overflow_drops"], 4,
        "forced external-sink channel overflow drop must be observable in the snapshot"
    );
}
