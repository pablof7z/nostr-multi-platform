//! `schema_version` + `last_tick_ms` liveness heartbeat projection.

use super::projection_fixtures_support::snapshot;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Every emitted snapshot MUST carry a `schema_version` field equal to the
/// canonical `SNAPSHOT_SCHEMA_VERSION`. Without it a version mismatch between a
/// shipped `.a` and the host fails silently — the host decodes renamed/removed
/// fields, gets wrong/null data, and shows a broken UI with no diagnostic
/// signal. This pins the field's presence on the actual on-wire bytes.
#[test]
fn snapshot_carries_schema_version() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let snap = snapshot(&mut kernel);
    assert_eq!(
        snap["schema_version"].as_u64(),
        Some(u64::from(crate::update_envelope::SNAPSHOT_SCHEMA_VERSION)),
        "every snapshot must stamp the canonical schema_version",
    );
}

/// Every emitted snapshot MUST carry a non-zero `last_tick_ms` (Unix-epoch
/// milliseconds), and the value MUST advance across successive emissions. A
/// shell watches this field to detect actor-thread death: a `dispatch_command`
/// panic is deliberately not caught, so it manifests as the update channel
/// going permanently silent. A frozen `last_tick_ms` is the only observable
/// signal of that otherwise-invisible freeze. This pins both the field's
/// presence on the on-wire bytes and its monotonic advance.
#[test]
fn snapshot_carries_advancing_last_tick_ms() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let first = snapshot(&mut kernel);
    let first_tick = first["last_tick_ms"]
        .as_u64()
        .expect("every snapshot must stamp a numeric last_tick_ms");
    assert!(
        first_tick > 0,
        "last_tick_ms must be a real Unix-epoch millisecond stamp, not zero",
    );

    let second = snapshot(&mut kernel);
    let second_tick = second["last_tick_ms"]
        .as_u64()
        .expect("every snapshot must stamp a numeric last_tick_ms");
    assert!(
        second_tick >= first_tick,
        "last_tick_ms must advance (or hold) across emissions, never regress; \
         a frozen value is the actor-thread-death signal",
    );
}
