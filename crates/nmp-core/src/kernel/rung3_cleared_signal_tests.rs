//! ADR-0070 Rung 3 S1b cleared-signal regression tests.
//!
//! Drives the full incremental-apply path for drain and copy-with-TTL keys:
//! `Changed` non-empty, exactly one empty `Cleared`, then absent/unchanged.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::super::clock::FixedClock;
use super::super::snapshot_registry::new_snapshot_projection_slot;
use super::super::Kernel;
use crate::kernel::action_ledger::RECENT_TERMINAL_TTL_MS;
use crate::kernel::action_stages::ActionStage;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::update_envelope::{
    decode_snapshot_typed_projections, TypedProjectionData, WireProjectionState,
};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Construct a fresh kernel with incremental-apply declared.
fn kernel_incremental() -> (
    Kernel,
    super::super::snapshot_registry::SnapshotProjectionSlot,
) {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));
    {
        let mut registry = slot.lock().expect("registry lock");
        registry.declare_incremental_apply();
    }
    (kernel, slot)
}

/// Emit one frame and return the decoded typed sidecar.
fn emit(kernel: &mut Kernel) -> Vec<TypedProjectionData> {
    let frame = kernel.make_update(true);
    decode_snapshot_typed_projections(&frame).unwrap_or_default()
}

/// A minimal in-test host cache stand-in (same algorithm as §3 D3-3).
/// `Changed` → insert/overwrite; `Cleared` → remove; absent/Unchanged → keep.
fn apply_to_cache(cache: &mut HashMap<String, Vec<u8>>, rows: &[TypedProjectionData]) {
    for row in rows {
        match row.state {
            WireProjectionState::Changed => {
                cache.insert(row.key.clone(), row.payload.clone());
            }
            WireProjectionState::Cleared => {
                cache.remove(&row.key);
            }
        }
    }
}

/// Find the first row for `key` in `rows`.
fn find_row<'a>(rows: &'a [TypedProjectionData], key: &str) -> Option<&'a TypedProjectionData> {
    rows.iter().find(|r| r.key == key)
}

// ── Drain key: action_results ─────────────────────────────────────────────────

/// `action_results` drain: non-empty → empty transition emits exactly one
/// Cleared row (frame B), then settles to Unchanged (frame C = absent).
///
/// Also verifies that the Cleared row removes the key from the host cache.
#[test]
fn action_results_cleared_on_drain_empty() {
    let (mut kernel, _slot) = kernel_incremental();

    // Baseline frame — consume it (declares incremental + resets last_emitted).
    let baseline = emit(&mut kernel);
    // Confirm action_results absent on a fresh kernel (no settlements yet).
    assert!(
        find_row(&baseline, "action_results").is_none(),
        "action_results must be absent on a fresh kernel baseline"
    );

    // Seed a settlement so action_results appears as Changed.
    kernel.record_action_success("corr-1".to_string(), None);
    let frame_a = emit(&mut kernel);
    let row_a = find_row(&frame_a, "action_results")
        .expect("action_results must be Changed (non-empty drain) in frame A");
    assert_eq!(
        row_a.state,
        WireProjectionState::Changed,
        "frame A: action_results must be Changed"
    );
    assert!(
        !row_a.payload.is_empty(),
        "frame A: action_results must have non-empty payload"
    );

    // Apply frame A to simulated host cache.
    let mut host_cache: HashMap<String, Vec<u8>> = HashMap::new();
    apply_to_cache(&mut host_cache, &frame_a);
    assert!(
        host_cache.contains_key("action_results"),
        "host cache must contain action_results after frame A"
    );

    // Frame B: drain is now empty (settlement consumed in frame A).
    // The S1b inverse pass MUST synthesize a Cleared row.
    let frame_b = emit(&mut kernel);
    let row_b = find_row(&frame_b, "action_results")
        .expect("action_results Cleared row MUST appear in frame B (S1b regression)");
    assert_eq!(
        row_b.state,
        WireProjectionState::Cleared,
        "frame B: action_results must be Cleared"
    );
    assert!(
        row_b.payload.is_empty(),
        "frame B: Cleared row must have empty payload"
    );

    // Apply frame B to simulated host cache — key must be evicted.
    apply_to_cache(&mut host_cache, &frame_b);
    assert!(
        !host_cache.contains_key("action_results"),
        "host cache must NOT contain action_results after Cleared in frame B"
    );

    // Frame C: action_results must be absent (settled to Unchanged — fires once).
    let frame_c = emit(&mut kernel);
    assert!(
        find_row(&frame_c, "action_results").is_none(),
        "action_results must be ABSENT in frame C (Unchanged after Cleared)"
    );
}

// ── Drain key: signed_events ──────────────────────────────────────────────────

/// `signed_events` drain: same Changed → Cleared → absent tristate.
#[test]
fn signed_events_cleared_on_drain_empty() {
    let (mut kernel, _slot) = kernel_incremental();
    let _ = emit(&mut kernel); // baseline

    // Seed a signed_event return.
    kernel.record_signed_event_return("corr-2", Ok(r#"{"id":"abc"}"#.to_string()));
    let frame_a = emit(&mut kernel);
    let row_a =
        find_row(&frame_a, "signed_events").expect("signed_events must be Changed in frame A");
    assert_eq!(row_a.state, WireProjectionState::Changed);
    assert!(!row_a.payload.is_empty());

    let mut host_cache: HashMap<String, Vec<u8>> = HashMap::new();
    apply_to_cache(&mut host_cache, &frame_a);
    assert!(host_cache.contains_key("signed_events"));

    // Frame B: drain empty → Cleared must appear.
    let frame_b = emit(&mut kernel);
    let row_b = find_row(&frame_b, "signed_events")
        .expect("signed_events Cleared row MUST appear in frame B (S1b regression)");
    assert_eq!(row_b.state, WireProjectionState::Cleared);
    assert!(row_b.payload.is_empty());

    apply_to_cache(&mut host_cache, &frame_b);
    assert!(!host_cache.contains_key("signed_events"));

    // Frame C: absent.
    let frame_c = emit(&mut kernel);
    assert!(find_row(&frame_c, "signed_events").is_none());
}

// ── Copy-with-TTL key: action_stages (finding 7 gate) ────────────────────────

/// `action_stages`: record a stage, ack it (removes last entry), then assert:
/// - Frame A: `action_stages` present as Changed.
/// - Frame B: `action_stages` present as Cleared (S1b §10.4 — finding 7).
/// - Frame C: absent.
///
/// THIS IS THE FINDING-7 REGRESSION: on current master (pre-S1b) the manifest
/// stays Unchanged or Changed-but-absent after ack-of-last-entry, and NO
/// Cleared row is emitted in frame B. The test must fail on master.
#[test]
fn action_stages_cleared_after_ack_of_last_entry() {
    let (mut kernel, _slot) = kernel_incremental();
    let _ = emit(&mut kernel); // baseline

    // Record one stage for correlation_id "c1".
    kernel.record_action_stage("c1", ActionStage::Requested, None);
    let frame_a = emit(&mut kernel);
    let row_a = find_row(&frame_a, "action_stages")
        .expect("action_stages must be Changed in frame A (non-empty)");
    assert_eq!(
        row_a.state,
        WireProjectionState::Changed,
        "frame A: action_stages must be Changed"
    );
    assert!(
        !row_a.payload.is_empty(),
        "frame A: action_stages must have non-empty payload"
    );

    let mut host_cache: HashMap<String, Vec<u8>> = HashMap::new();
    apply_to_cache(&mut host_cache, &frame_a);
    assert!(host_cache.contains_key("action_stages"));

    // Ack the last (only) entry — tracker becomes empty.
    kernel.ack_action_stage("c1");

    // Frame B: S1b §10.4 Cleared-edge machine (note_copy_emit) must park
    // Cleared → synthesis emits the Cleared row.
    let frame_b = emit(&mut kernel);
    let row_b = find_row(&frame_b, "action_stages").expect(
        "action_stages Cleared row MUST appear in frame B after ack-of-last-entry \
                 (S1b / finding 7 regression)",
    );
    assert_eq!(
        row_b.state,
        WireProjectionState::Cleared,
        "frame B: action_stages must be Cleared after ack-of-last-entry"
    );
    assert!(
        row_b.payload.is_empty(),
        "frame B: Cleared row must have empty payload"
    );

    apply_to_cache(&mut host_cache, &frame_b);
    assert!(
        !host_cache.contains_key("action_stages"),
        "host cache must NOT contain action_stages after Cleared in frame B"
    );

    // Frame C: absent.
    let frame_c = emit(&mut kernel);
    assert!(
        find_row(&frame_c, "action_stages").is_none(),
        "action_stages must be ABSENT in frame C (Unchanged after Cleared)"
    );
}

/// `action_stages`: terminal retention expiry must also synthesize a Cleared
/// row. This is the no-host-ack path: the only event after the terminal emit is
/// advancing the injected kernel clock and producing another snapshot.
#[test]
fn action_stages_cleared_after_ttl_expiry_without_ack() {
    let epoch = SystemTime::UNIX_EPOCH;
    let (mut kernel, _slot) = kernel_incremental();
    kernel.set_clock(Arc::new(FixedClock(epoch)));
    let _ = emit(&mut kernel); // baseline

    kernel.record_action_stage("c-stage-ttl", ActionStage::Accepted, None);
    let frame_a = emit(&mut kernel);
    let row_a = find_row(&frame_a, "action_stages")
        .expect("action_stages must be Changed in frame A (terminal retained)");
    assert_eq!(row_a.state, WireProjectionState::Changed);
    assert!(!row_a.payload.is_empty());

    let mut host_cache: HashMap<String, Vec<u8>> = HashMap::new();
    apply_to_cache(&mut host_cache, &frame_a);
    assert!(host_cache.contains_key("action_stages"));

    let past_ttl = epoch + Duration::from_millis(RECENT_TERMINAL_TTL_MS);
    kernel.set_clock(Arc::new(FixedClock(past_ttl)));

    let frame_b = emit(&mut kernel);
    let row_b = find_row(&frame_b, "action_stages")
        .expect("action_stages Cleared row must appear after terminal TTL expiry");
    assert_eq!(row_b.state, WireProjectionState::Cleared);
    assert!(row_b.payload.is_empty());

    apply_to_cache(&mut host_cache, &frame_b);
    assert!(!host_cache.contains_key("action_stages"));

    let frame_c = emit(&mut kernel);
    assert!(find_row(&frame_c, "action_stages").is_none());
}

// ── Copy-with-TTL key: action_lifecycle (TTL expiry path) ────────────────────

/// `action_lifecycle`: record a terminal stage (which populates the lifecycle
/// tracker), then advance the `FixedClock` past the TTL — the snapshot prunes
/// the expired terminal, making the tracker empty → Cleared.
#[test]
fn action_lifecycle_cleared_after_ttl_expiry() {
    // Start the clock at t=0.
    let epoch = SystemTime::UNIX_EPOCH;
    let (mut kernel, _slot) = kernel_incremental();
    kernel.set_clock(Arc::new(FixedClock(epoch)));
    let _ = emit(&mut kernel); // baseline

    // Record a terminal stage (Accepted) at t=0. This populates action_lifecycle.
    kernel.record_action_success("c-lc".to_string(), None);
    let frame_a = emit(&mut kernel);
    let row_a = find_row(&frame_a, "action_lifecycle")
        .expect("action_lifecycle must be Changed in frame A (non-empty terminal)");
    assert_eq!(
        row_a.state,
        WireProjectionState::Changed,
        "frame A: action_lifecycle must be Changed"
    );
    assert!(
        !row_a.payload.is_empty(),
        "frame A: action_lifecycle must have non-empty payload"
    );

    let mut host_cache: HashMap<String, Vec<u8>> = HashMap::new();
    apply_to_cache(&mut host_cache, &frame_a);
    assert!(host_cache.contains_key("action_lifecycle"));

    // Advance the clock past the terminal TTL so the next snapshot prunes it.
    let past_ttl = epoch + Duration::from_millis(RECENT_TERMINAL_TTL_MS + 1);
    kernel.set_clock(Arc::new(FixedClock(past_ttl)));

    // Frame B: snapshot prunes the expired terminal → tracker empty → note_copy_emit
    // parks Cleared → synthesis emits the Cleared row.
    let frame_b = emit(&mut kernel);
    let row_b = find_row(&frame_b, "action_lifecycle").expect(
        "action_lifecycle Cleared row MUST appear in frame B after TTL expiry \
                 (S1b / finding 7 regression for action_lifecycle)",
    );
    assert_eq!(
        row_b.state,
        WireProjectionState::Cleared,
        "frame B: action_lifecycle must be Cleared after TTL expiry"
    );
    assert!(
        row_b.payload.is_empty(),
        "frame B: Cleared row must have empty payload"
    );

    apply_to_cache(&mut host_cache, &frame_b);
    assert!(
        !host_cache.contains_key("action_lifecycle"),
        "host cache must NOT contain action_lifecycle after Cleared in frame B"
    );

    // Frame C: absent.
    let frame_c = emit(&mut kernel);
    assert!(
        find_row(&frame_c, "action_lifecycle").is_none(),
        "action_lifecycle must be ABSENT in frame C (Unchanged after Cleared)"
    );
}

// ── Spurious-clear negative test ──────────────────────────────────────────────

/// A key that is empty for the ENTIRE run must NEVER produce a Cleared row.
/// `action_results` with zero settlements stays at `Unchanged` forever.
///
/// This verifies the edge machine does NOT fire on `was_empty && !nonempty`
/// and the inverse pass does NOT synthesize for `Unchanged`-absent keys.
#[test]
fn always_empty_key_never_produces_cleared_row() {
    let (mut kernel, _slot) = kernel_incremental();
    let _ = emit(&mut kernel); // baseline

    // Emit several frames with no settlements.
    for _ in 0..5 {
        let frame = emit(&mut kernel);
        for key in &[
            "action_results",
            "signed_events",
            "action_stages",
            "action_lifecycle",
        ] {
            if let Some(row) = find_row(&frame, key) {
                // If the row appears at all, it must NOT be Cleared (it was never populated).
                assert_ne!(
                    row.state,
                    WireProjectionState::Cleared,
                    "key `{key}` was always empty — must NOT produce a Cleared row; \
                     but got state={:?}",
                    row.state
                );
            }
        }
    }
}

// ── Multiple entries: partial ack does not clear ──────────────────────────────

/// When action_stages has TWO entries and one is acked, the remaining entry
/// means the tracker is NOT empty → no Cleared row → key delivered as Changed
/// EXACTLY ONCE on the next tick (the ack is a genuine content edit so the rev
/// MUST advance — #1390 review FIX 2), then settles to absent (Unchanged).
#[test]
fn action_stages_partial_ack_stays_changed() {
    let (mut kernel, _slot) = kernel_incremental();
    let _ = emit(&mut kernel); // baseline

    kernel.record_action_stage("c1", ActionStage::Requested, None);
    kernel.record_action_stage("c2", ActionStage::Requested, None);
    let frame_a = emit(&mut kernel); // frame A — both entries
    let row_a = find_row(&frame_a, "action_stages")
        .expect("frame A: action_stages must be Changed (both entries)");
    assert_eq!(row_a.state, WireProjectionState::Changed);
    let payload_two = row_a.payload.clone();

    // Ack only c1; c2 remains. The reduced mirror is a different payload.
    kernel.ack_action_stage("c1");
    let frame_b = emit(&mut kernel);

    // FIX 2: a partial ack bumps settlement_enqueue_ver, so the rev advances and
    // the reduced (c2-only) mirror is delivered as Changed EXACTLY ONCE — not
    // omitted (which would leave the host caching the stale two-entry mirror),
    // and not Cleared (entries remain).
    let row_b = find_row(&frame_b, "action_stages").expect(
        "frame B: partial ack MUST deliver action_stages as Changed exactly once \
         (rev advanced via settlement_enqueue_ver — #1390 FIX 2)",
    );
    assert_eq!(
        row_b.state,
        WireProjectionState::Changed,
        "partial ack: action_stages must be Changed (c2 still tracked)"
    );
    assert_ne!(
        row_b.payload, payload_two,
        "partial ack: the reduced (c2-only) payload must differ from the two-entry payload"
    );

    // Frame C: no further mutation → action_stages settles to absent (Unchanged).
    let frame_c = emit(&mut kernel);
    assert!(
        find_row(&frame_c, "action_stages").is_none(),
        "frame C: action_stages must be ABSENT once the partial-ack Changed frame is delivered"
    );
}

// ── Steady-state omission (the FIX 1 regression gate) ─────────────────────────

/// **FIX 1 regression gate (#1390 review).** A STABLE non-empty
/// `action_stages` / `action_lifecycle` (an in-flight action whose content does
/// not change tick-to-tick) must be delivered as Changed EXACTLY ONCE, then
/// OMITTED (absent) on every subsequent tick.
///
/// On the PR head before FIX 1, `note_copy_emit` parked `Changed` into
/// `pending_presence` on every non-empty tick, so `presence_for` returned the
/// parked value before the rev-vs-last-emit rule could settle it — the full
/// payload re-emitted on every 4Hz tick forever. This test FAILS on that head
/// (it would find a row on ticks 2..N) and PASSES once FIX 1 restricts parking
/// to the Cleared edge only.
///
/// This mirrors the finding-9 discipline that made the cleared-signal gate real:
/// prove the OPTIMIZED path (omission), not just the correctness path (Cleared).
#[test]
fn stable_nonempty_copy_keys_omitted_after_first_changed() {
    let (mut kernel, _slot) = kernel_incremental();
    let epoch = SystemTime::UNIX_EPOCH;
    // Pin the clock so action_lifecycle's terminal never crosses its TTL during
    // the probe — we want a genuinely STABLE non-empty projection, not a TTL edge.
    kernel.set_clock(Arc::new(FixedClock(epoch)));
    let _ = emit(&mut kernel); // baseline

    // Populate BOTH copy-with-TTL keys with a stable, non-terminal-expiring
    // payload: an in-flight (non-terminal) stage drives action_stages, and a
    // terminal success drives action_lifecycle (frozen clock keeps it alive).
    kernel.record_action_stage("inflight", ActionStage::Requested, None);
    kernel.record_action_success("done".to_string(), None);

    // Frame 1: both keys delivered as Changed exactly once.
    let frame_1 = emit(&mut kernel);
    for key in ["action_stages", "action_lifecycle"] {
        let row = find_row(&frame_1, key)
            .unwrap_or_else(|| panic!("frame 1: {key} must be Changed (freshly populated)"));
        assert_eq!(
            row.state,
            WireProjectionState::Changed,
            "frame 1: {key} must be Changed on first non-empty emit"
        );
    }

    // Frames 2..=6: NO mutation, clock frozen → content is byte-identical.
    // Each key MUST be OMITTED (absent). A present row on any of these ticks is
    // the perpetual-Changed byte leak FIX 1 closes.
    for tick in 2..=6 {
        let frame = emit(&mut kernel);
        for key in ["action_stages", "action_lifecycle"] {
            assert!(
                find_row(&frame, key).is_none(),
                "tick {tick}: {key} must be OMITTED on a stable non-empty tick \
                 (perpetual-Changed byte leak — #1390 FIX 1). Found a row instead."
            );
        }
    }
}
