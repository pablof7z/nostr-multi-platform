//! D5 — registration-count ceilings for the snapshot-projection registry.
//!
//! D5 requires that snapshot output is bounded by the set of currently-open
//! views. An unbounded registry would let a misconfigured or adversarial host
//! grow the per-tick serialisation cost without limit, violating D5 and D8
//! (snapshot cost must be proportional to open views). These constants and the
//! [`admit_keyed`] / [`admit_additive`] helpers are the runtime enforcement of
//! that bound; the [`super::SnapshotRegistry`] `register*` methods call them
//! before inserting.

/// D5 — maximum number of distinct keys in the generic (`projections`) or
/// typed (`typed_projections`) projection registries.
///
/// The busiest in-repo app (the Chirp defaults layer, counting across
/// `nmp-defaults`, `nmp-nip47`, `nmp-nip17`, `nmp-marmot`, `nmp-nip02`,
/// `nmp-nip29`, `nmp-content`, `nmp-wot`) registers approximately **12**
/// distinct projection keys. A 4× headroom factor (48, rounded up to 64 for
/// alignment) yields a defensible ceiling that absorbs future growth while
/// still preventing accidental unbounded accumulation.
///
/// Registration of a new key beyond this limit is a **loud no-op** (a
/// `tracing::warn!` line is emitted; the closure is silently dropped). This
/// follows the D6 contract: configuration-time errors are never panics, but
/// they MUST be observable.
pub const MAX_SNAPSHOT_PROJECTIONS: usize = 64;

/// D5 — maximum number of per-tick observer closures in the `tick_observers`
/// list.
///
/// Tick observers are additive (no key dedup), so they have their own bound.
/// Production wires exactly **1** today; 16 gives generous headroom while
/// capping runaway registration.
pub const MAX_TICK_OBSERVERS: usize = 16;

/// Decide whether a **keyed** registration may be admitted.
///
/// Returns `true` when the registration should proceed: either the key already
/// exists (re-registration / last-writer-wins is always allowed) or the
/// registry is below [`MAX_SNAPSHOT_PROJECTIONS`]. Returns `false` (and emits a
/// `tracing::warn!` line — D6 loud no-op) when a **new** key would push the
/// registry past the ceiling.
///
/// `registry` names the registry for the diagnostic (`"snapshot projection"` /
/// `"typed snapshot projection"`).
pub(super) fn admit_keyed(
    len: usize,
    key_exists: bool,
    key: &str,
    registry: &str,
) -> bool {
    if !key_exists && len >= MAX_SNAPSHOT_PROJECTIONS {
        tracing::warn!(
            key = %key,
            limit = MAX_SNAPSHOT_PROJECTIONS,
            "D5: {registry} registry is full — registration of '{key}' \
             dropped (limit: {MAX_SNAPSHOT_PROJECTIONS})"
        );
        return false;
    }
    true
}

/// Decide whether an **additive** (tick-observer) registration may be admitted.
///
/// Returns `false` (loud no-op) once the list is at [`MAX_TICK_OBSERVERS`].
pub(super) fn admit_additive(len: usize) -> bool {
    if len >= MAX_TICK_OBSERVERS {
        tracing::warn!(
            limit = MAX_TICK_OBSERVERS,
            "D5: tick-observer list is full — registration dropped \
             (limit: {MAX_TICK_OBSERVERS})"
        );
        return false;
    }
    true
}
