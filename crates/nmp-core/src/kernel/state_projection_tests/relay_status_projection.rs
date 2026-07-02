//! Relay connection events → `relay_status` / `relay_statuses[]` projection.
//!
//! D0 note: NIP-47 NWC is an app noun — wallet state is NO LONGER a typed
//! `KernelSnapshot` field. It is surfaced through the `"wallet"`
//! host-registered snapshot projection. The connect / disconnect lifecycle
//! proof lives with the other snapshot-projection tests in
//! `snapshot_registry_tests.rs`
//! (`wallet_projection_appears_and_clears_through_make_update`), since it now
//! exercises the projection seam rather than a kernel-owned field.

use super::support::snapshot;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_network::role::RelayRole;

/// A relay connection transition must surface in the snapshot's `relay_status`
/// (the headline content relay) and `relay_statuses[]` (every lane). A
/// projection that read a stale field would show "disconnected" after a real
/// connect — exactly the kind of display bug this layer must not have.
#[test]
fn relay_status_appears_in_snapshot_after_connection_events() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // `start()` seeds `started_at` so `elapsed_ms` (and thus
    // `last_connected_at_ms`) can resolve a real timestamp.
    kernel.start();

    // Default lane state: not connected.
    let before = snapshot(&mut kernel);
    assert_ne!(
        before["relay_status"]["connection"].as_str(),
        Some("connected"),
        "a fresh content relay lane must not project as connected",
    );

    // Drive the connecting → connected transition on the content lane.
    kernel.relay_connecting(RelayRole::Content);
    let connecting = snapshot(&mut kernel);
    assert_eq!(
        connecting["relay_status"]["connection"].as_str(),
        Some("connecting"),
        "relay_connecting must project `connecting` onto relay_status",
    );

    kernel.relay_connected(RelayRole::Content);
    let connected = snapshot(&mut kernel);
    assert_eq!(
        connected["relay_status"]["connection"].as_str(),
        Some("connected"),
        "relay_connected must project `connected` onto relay_status",
    );
    assert!(
        connected["relay_status"]["last_connected_at_ms"].is_u64(),
        "a connected relay must project a numeric last_connected_at_ms",
    );

    // The content lane must also be present (and connected) in relay_statuses[].
    let statuses = connected["relay_statuses"]
        .as_array()
        .expect("relay_statuses must be a JSON array");
    let content = statuses
        .iter()
        .find(|s| s["role"].as_str() == Some("content"))
        .expect("relay_statuses must include the content lane");
    assert_eq!(
        content["connection"].as_str(),
        Some("connected"),
        "the content lane in relay_statuses[] must agree with relay_status",
    );

    // A subsequent close must project back to a non-connected state — a
    // projection stuck on the stale `connected` value is the bug under test.
    // (`relay_closed_all` — the global-teardown path — projects the lane
    // `closed` regardless of per-URL socket bookkeeping.)
    kernel.relay_closed_all(RelayRole::Content);
    let closed = snapshot(&mut kernel);
    assert_eq!(
        closed["relay_status"]["connection"].as_str(),
        Some("closed"),
        "relay_closed must project `closed`, never a stale `connected`",
    );
}
