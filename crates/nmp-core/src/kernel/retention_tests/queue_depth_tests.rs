//! G-S4 — the actor command-channel depth counter (`queue_depth`) round-trips
//! through the kernel snapshot accessor.

use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::atomic::{AtomicU64, Ordering};

/// The `Arc<AtomicU64>` shared with `NmpApp::send_cmd` must reach
/// `actor_queue_depth()` so the snapshot surfaces real command-channel
/// occupancy, and must survive `Reset`.
#[test]
fn queue_depth_handle_surfaces_on_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert_eq!(
        kernel.actor_queue_depth(),
        0,
        "unbound kernel reports zero queue depth"
    );

    let handle = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_queue_depth_handle(std::sync::Arc::clone(&handle));

    // External mutation (mirrors `NmpApp::send_cmd` incrementing on send).
    handle.fetch_add(7, Ordering::Relaxed);
    assert_eq!(
        kernel.actor_queue_depth(),
        7,
        "kernel must observe external Arc increments"
    );

    // Saturation: a count above u32::MAX collapses to u32::MAX, not a wrap.
    handle.store(u64::from(u32::MAX) + 100, Ordering::Relaxed);
    assert_eq!(
        kernel.actor_queue_depth(),
        u32::MAX,
        "queue depth saturates at u32::MAX"
    );
    handle.store(7, Ordering::Relaxed);

    // Reset round-trip: extract → reinstall onto fresh kernel.
    let extracted = kernel.take_queue_depth_handle_for_reset();
    assert!(extracted.is_some());
    let mut fresh = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    fresh.set_queue_depth_handle(extracted.unwrap());
    handle.fetch_add(1, Ordering::Relaxed);
    assert_eq!(
        fresh.actor_queue_depth(),
        8,
        "Reset must preserve the queue-depth counter via take→set round-trip"
    );
}
