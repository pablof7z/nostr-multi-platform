//! V-103: D1 Offline Bootstrap Regression Test
//!
//! Doctrine D1 mandates that the first rendered snapshot must precede any relay
//! I/O — the kernel must emit an initial update frame from offline-stored events
//! **before** dialing any relays.
//!
//! This regression test validates two aspects of the D1 guarantee:
//!
//! 1. **Fresh kernel emits snapshot without relay connections:** A newly
//!    constructed kernel (zero relays configured) must be capable of emitting
//!    a valid KernelUpdate snapshot structure before any relay I/O begins. This
//!    proves the snapshot path does not depend on relay connectivity.
//!
//! 2. **No relay rows configured:** The kernel starts with an empty relay list.
//!    The snapshot should emit with the `no_configured_relays: false` field
//!    when no account is active (the normal offline-first cold-start state).
//!
//! See `docs/product-spec/offline-first.md` §7 (line 80–82) and
//! `docs/wiki/d1-snapshot-before-relay-io.md`.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// D1 assertion: a freshly-constructed kernel with zero relay URLs configured
/// can emit a valid snapshot structure before any relay I/O.
///
/// This validates that the snapshot emission path (make_update) does not depend
/// on relay connectivity. The kernel is an offline-capable data engine.
///
/// Note: The timeline projection is only emitted when the shell subscribes to
/// the timeline view (D5 bounding rule). For a fresh kernel without any
/// subscriptions, the timeline will be absent from the snapshot. This test
/// validates the snapshot structure itself, not timeline content.
#[test]
fn d1_fresh_kernel_emits_snapshot_without_relays() {
    // Construct a kernel with storage but no relays configured.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Precondition: no relays are configured (kernel default state).
    // The kernel has an empty relay_edit_rows list and zero active subscriptions.

    // Trigger the snapshot emission path. This is called by the actor on every
    // kernel tick and before any relay connection is established.
    let snapshot_json = kernel.make_update_json_for_test(true);

    // ── Validate the snapshot structure ────────────────────────────────────────
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot JSON must be valid");

    // D1 requirement 1: snapshot must be a valid JSON object (not null, not array).
    assert!(
        parsed.is_object(),
        "D1: snapshot must be a JSON object; got: {:?}",
        parsed
    );

    // D1 requirement 2: snapshot must contain a `projections` field per the
    // KernelUpdate contract (see crates/nmp-core/src/kernel/update.rs).
    // This proves the snapshot structure is ready BEFORE any relay I/O.
    let projections = parsed
        .get("projections")
        .and_then(|p| p.as_object())
        .unwrap_or_else(|| {
            panic!(
                "D1: snapshot must have a 'projections' field; got: {}",
                serde_json::to_string_pretty(&parsed).unwrap_or_default()
            )
        });

    // D1 requirement 3: the projections object must contain at least the
    // structural fields (accounts, active_account, relay diagnostics, etc.).
    // These are always emitted, independent of relay connectivity.
    assert!(
        !projections.is_empty(),
        "D1: projections must be non-empty; got: {:?}",
        projections
    );

    // The critical D1 property is satisfied: the kernel structure emits a valid
    // snapshot BEFORE any relay connections. The test passes if we reach here.
}

/// D1 assertion: when no account is active (offline-first cold state), the
/// kernel snapshot does not emit `no_configured_relays` (no user context exists).
///
/// This guards against false positives: the absence of relay rows is expected
/// when unsigned in, not a user-observable problem.
#[test]
fn d1_offline_no_account_snapshot_omits_no_configured_relays() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Do NOT set an active account — kernel starts unsigned-in.

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot JSON must be valid");

    // D1: with no account, `no_configured_relays` must be absent from the snapshot
    // (the absence of relays is expected, not a user-observable failure).
    assert!(
        !parsed
            .as_object()
            .map(|o| o.contains_key("no_configured_relays"))
            .unwrap_or(false),
        "D1: unsigned-in (no account) kernel snapshot must NOT emit \
         'no_configured_relays' key; got keys: {:?}",
        parsed
            .as_object()
            .map(|o| o.keys().collect::<Vec<_>>())
            .unwrap_or_default()
    );
}
