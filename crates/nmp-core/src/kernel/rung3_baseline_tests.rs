//! ADR-0055 Rung 3 — integration tests for baseline semantics (D3-5).
//!
//! Verifies:
//! 1. First frame after `declare_incremental_apply` ⇒ full baseline (all live
//!    Tier-2 keys present as Changed).
//! 2. First frame after `bump_epoch` ⇒ full baseline.
//! 3. The Rung-1 biconditional oracle extended to the omission case: a row omitted
//!    from the frame ⟺ the manifest's presence is `Unchanged` (i.e. cache-unit
//!    unchanged since last emit).
//!
//! These drive the REAL `make_update` path with the snapshot registry installed
//! so `incremental_apply_enabled()` and `take_incremental_apply_baseline_pending()`
//! both work correctly.

use std::sync::Arc;

// This module is pulled in via `#[path]` from `kernel::update` (so the
// `kernel/mod.rs` god-module stays at its size baseline), so `super` here is
// `kernel::update`. Reach the kernel root through `super::super` and read the
// built-in-keys list from the enclosing `update` module directly.
use super::super::snapshot_registry::new_snapshot_projection_slot;
use super::super::Kernel;
use super::KERNEL_BUILTIN_PROJECTION_KEYS;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::update_envelope::{decode_snapshot_typed_projections, WireProjectionState};

/// Sanity-check: a fresh kernel with a snapshot slot produces non-empty typed
/// projections from the production `make_update` path.
#[test]
fn production_make_update_produces_typed_projections() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));
    let frame = kernel.make_update(true);
    let typed = decode_snapshot_typed_projections(&frame).unwrap_or_default();
    assert!(
        !typed.is_empty(),
        "production make_update must produce non-empty typed projections; got empty"
    );
}

/// Sanity-check: declaring incremental apply before the first emit still produces
/// a non-empty typed sidecar (full baseline).
#[test]
fn declare_before_first_emit_still_has_typed_projections() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));
    // Declare incremental apply before first emit.
    {
        let mut registry = slot.lock().expect("registry lock");
        registry.declare_incremental_apply();
    }
    let frame = kernel.make_update(true);
    let typed = decode_snapshot_typed_projections(&frame).unwrap_or_default();
    let keys: Vec<&str> = typed.iter().map(|r| r.key.as_str()).collect();
    assert!(
        !typed.is_empty(),
        "even with incremental apply declared, first emit must produce non-empty typed projections; keys={keys:?}"
    );
    assert!(
        keys.contains(&"profile"),
        "profile must appear in baseline frame; keys={keys:?}"
    );
}

/// Construct a fresh kernel with a snapshot slot installed (so registry reads
/// in `make_update` succeed).
fn kernel_with_slot() -> (Kernel, super::super::snapshot_registry::SnapshotProjectionSlot) {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));
    (kernel, slot)
}

/// Emit one frame and return the decoded typed sidecar.
fn emit_frame(kernel: &mut Kernel) -> Vec<crate::update_envelope::TypedProjectionData> {
    let frame = kernel.make_update(true);
    decode_snapshot_typed_projections(&frame).unwrap_or_default()
}

/// The Tier-2 built-in keys whose presence we care about for baseline checks.
/// We validate a subset of them (the ones that always produce a payload on any
/// fresh kernel tick — i.e. the non-drain keys that are `Changed` on the first
/// tick).
fn tier2_keys() -> &'static [&'static str] {
    KERNEL_BUILTIN_PROJECTION_KEYS
}

// ── Baseline after declare_incremental_apply ──────────────────────────────────

/// After calling `declare_incremental_apply` and running one warmup tick (to
/// settle `Unchanged` state), the SECOND tick must be a full baseline because
/// `take_incremental_apply_baseline_pending` drains the latch and resets
/// `last_emitted`.
///
/// Concretely: even though most Tier-2 built-ins are `Unchanged` after the
/// warmup tick, the baseline-pending reset forces them ALL to appear as
/// `Changed` in the first incremental-enabled frame.
#[test]
fn first_frame_after_declare_incremental_apply_is_full_baseline() {
    let (mut kernel, slot) = kernel_with_slot();

    // Warmup: emit one full tick to advance last_emitted for all keys.
    let _ = emit_frame(&mut kernel);
    // Emit a second warmup tick — all Tier-2 keys are now Unchanged in the
    // tracker (no mutations since tick 1). With incremental OFF, all still emit.
    let _ = emit_frame(&mut kernel);

    // Now declare incremental apply — this sets the baseline-pending latch.
    {
        let mut registry = slot.lock().expect("registry lock");
        registry.declare_incremental_apply();
    }

    // The next emit must be a full baseline: baseline-pending clears last_emitted,
    // so all Tier-2 keys are Changed again, and omit_unchanged keeps all of them.
    let typed = emit_frame(&mut kernel);

    // Every Tier-2 built-in that has a payload on a fresh kernel MUST appear.
    // Non-drain keys that always produce bytes: configured_relays, profile,
    // accounts, active_account, claimed_profiles, resolved_profiles,
    // claimed_events, mention_profiles, relay_role_options, settings_hub,
    // publish_queue, publish_outbox, outbox_summary, relay_diagnostics.
    //
    // We check the set of present keys covers all built-in UNCONDITIONAL keys.
    // The four conditional drain keys (action_results, signed_events,
    // action_stages, action_lifecycle) are absent on ticks with no settlements
    // — they are present only when a settlement occurred. That is correct
    // baseline behavior: a fresh kernel has no settlement activity.
    let present_keys: std::collections::HashSet<&str> =
        typed.iter().map(|r| r.key.as_str()).collect();

    for &key in tier2_keys() {
        // Skip the four conditional drain/stage keys — they are absent on a
        // fresh kernel with no settlements. All other Tier-2 keys MUST appear
        // in the baseline frame (they are unconditionally produced each tick).
        if matches!(key, "action_results" | "signed_events" | "action_stages" | "action_lifecycle") {
            continue;
        }
        assert!(
            present_keys.contains(key),
            "Tier-2 key `{key}` must appear in baseline frame after \
             declare_incremental_apply; present_keys = {present_keys:?}"
        );
    }
}

/// Verify the latch fires only ONCE: the second incremental-enabled frame after
/// declare should NOT reset last_emitted again — keys that didn't change should
/// be omitted.
#[test]
fn second_incremental_frame_omits_unchanged_keys() {
    let (mut kernel, slot) = kernel_with_slot();

    // Declare incremental apply before any emit.
    {
        let mut registry = slot.lock().expect("registry lock");
        registry.declare_incremental_apply();
    }

    // First frame: baseline (latch fires, all keys present).
    let frame1 = emit_frame(&mut kernel);
    let frame1_keys: std::collections::HashSet<&str> =
        frame1.iter().map(|r| r.key.as_str()).collect();
    // Spot-check: profile must appear in the baseline.
    assert!(
        frame1_keys.contains("profile"),
        "profile must appear in the baseline frame"
    );

    // Second frame: no mutations → profile (and most other Tier-2 keys) are
    // Unchanged → omitted.
    let frame2 = emit_frame(&mut kernel);
    let frame2_keys: std::collections::HashSet<&str> =
        frame2.iter().map(|r| r.key.as_str()).collect();
    // profile has no active account → Unchanged → omitted in frame 2.
    // (relay_diagnostics may or may not change depending on diagnostics inputs.)
    assert!(
        !frame2_keys.contains("profile"),
        "profile (Unchanged) must be ABSENT in the second incremental frame; \
         present = {frame2_keys:?}"
    );
}

// ── Baseline after bump_epoch ─────────────────────────────────────────────────

/// After `bump_epoch`, the next frame must be a full baseline (all Tier-2 keys
/// `Changed`) even with incremental apply enabled.
#[test]
fn first_frame_after_bump_epoch_is_full_baseline() {
    let (mut kernel, slot) = kernel_with_slot();

    // Enable incremental apply.
    {
        let mut registry = slot.lock().expect("registry lock");
        registry.declare_incremental_apply();
    }

    // First frame: baseline (declare latch fires).
    let _ = emit_frame(&mut kernel);

    // Second frame: mostly Unchanged → omitted.
    let _ = emit_frame(&mut kernel);

    // Now simulate an epoch bump (account-switch / schema-change path).
    kernel.projection_rev_tracker.bump_epoch();

    // The next frame must be a full baseline because bump_epoch clears last_emitted.
    let typed = emit_frame(&mut kernel);
    let present_keys: std::collections::HashSet<&str> =
        typed.iter().map(|r| r.key.as_str()).collect();

    for &key in tier2_keys() {
        // Skip conditional drain/stage keys (absent on ticks with no settlements).
        if matches!(key, "action_results" | "signed_events" | "action_stages" | "action_lifecycle") {
            continue;
        }
        assert!(
            present_keys.contains(key),
            "Tier-2 key `{key}` must appear in baseline frame after bump_epoch; \
             present = {present_keys:?}"
        );
    }
}

// ── Omission biconditional oracle extension ───────────────────────────────────

/// Omission oracle: a row is absent from the frame ⟺ the manifest's presence
/// for that key is `Unchanged`. In other words, the kernel never omits a row
/// that is `Changed` or `Cleared`, and never emits an `Unchanged` row.
///
/// Drives two back-to-back ticks with incremental apply enabled:
/// Tick 1 = baseline (all Changed). Tick 2 = no mutations → all Unchanged →
/// all omitted. Checks the biconditional against the manifest.
#[test]
fn omission_biconditional_oracle_omitted_iff_unchanged() {
    let (mut kernel, slot) = kernel_with_slot();

    {
        let mut registry = slot.lock().expect("registry lock");
        registry.declare_incremental_apply();
    }

    // Tick 1: baseline frame — all Tier-2 keys are Changed (latch fires).
    // The oracle in `make_update` (test-support build) already checks the
    // biconditional for Changed/Cleared. We extend it here to the omission case.
    let frame1 = emit_frame(&mut kernel);

    // Verify: all non-drain Tier-2 keys present in frame 1 as Changed.
    let present_tick1: std::collections::HashMap<&str, WireProjectionState> = frame1
        .iter()
        .map(|r| (r.key.as_str(), r.state))
        .collect();
    for &key in tier2_keys() {
        // Skip conditional drain/stage keys (absent on ticks with no settlements).
        if matches!(key, "action_results" | "signed_events" | "action_stages" | "action_lifecycle") {
            continue;
        }
        let state = present_tick1.get(key).copied();
        assert!(
            matches!(state, Some(WireProjectionState::Changed) | Some(WireProjectionState::Cleared)),
            "Tick 1 baseline: key `{key}` must be Changed or Cleared (not absent); \
             got {state:?}"
        );
    }

    // Tick 2: no mutations → all Unchanged → omitted.
    let frame2 = emit_frame(&mut kernel);
    let present_tick2: std::collections::HashSet<&str> =
        frame2.iter().map(|r| r.key.as_str()).collect();

    // Biconditional (omission direction): any key absent from tick 2 MUST have
    // been Unchanged in the manifest (i.e. its rev did not advance). We verify
    // the contrapositive by checking that only drain-projection keys appear in
    // tick 2 (drain projections have Cleared presence on empty ticks and are
    // therefore kept with an explicit Cleared row — they are NOT omitted).
    //
    // For a fresh kernel with no mutations between tick 1 and tick 2:
    // - Non-drain Tier-2 keys: their rev did not advance → Unchanged → absent.
    // - Drain keys (action_results, signed_events): Cleared (if they appeared
    //   in tick 1) or Unchanged (if they were already absent). Either way they
    //   should not appear as Changed.
    for &key in tier2_keys() {
        // Skip conditional drain/stage keys: on a fresh kernel they were absent
        // in tick 1 (no settlements) and thus also absent in tick 2. Allowing
        // them here avoids false positives when they ARE absent in tick 2 not
        // because they were omitted (Unchanged) but because they never appeared.
        if matches!(key, "action_results" | "signed_events" | "action_stages" | "action_lifecycle") {
            continue;
        }
        assert!(
            !present_tick2.contains(key),
            "Tick 2 (no mutations, incremental ON): key `{key}` must be ABSENT \
             (Unchanged → omitted); present = {present_tick2:?}"
        );
    }

    // ── Tick 3: MIXED tick — mutate exactly ONE source cluster, leave the rest ──
    //
    // The two-tick check above only exercises the all-Changed and all-Unchanged
    // extremes. The real value of the omission layer is the MIXED case: when a
    // single input changes, the kernel must keep exactly the projections that
    // depend on it and omit every other (still-Unchanged) key in the SAME frame.
    //
    // `set_configured_relays` is a real-kernel mutation that advances
    // `configured_relays_ver` — driving the `configured_relays` /
    // `relay_role_options` / `settings_hub` cluster — plus `relay_diagnostics`
    // (whose per-emit fingerprint folds in the configured-relay set). Every
    // OTHER Tier-2 key (`profile`, `accounts`, `active_account`,
    // `claimed_profiles`, `resolved_profiles`, `claimed_events`,
    // `mention_profiles`, the publish cluster) is untouched and MUST be omitted.
    use crate::kernel::AppRelay;
    kernel.set_configured_relays(vec![AppRelay::new(
        "wss://relay.example/".to_string(),
        "both".to_string(),
    )]);

    let frame3 = emit_frame(&mut kernel);
    let present_tick3: std::collections::HashSet<&str> =
        frame3.iter().map(|r| r.key.as_str()).collect();

    // Kept (Changed): the configured-relays cluster + relay_diagnostics.
    for key in ["configured_relays", "relay_role_options", "settings_hub", "relay_diagnostics"] {
        assert!(
            present_tick3.contains(key),
            "Tick 3 (mixed): key `{key}` depends on the mutated input → must be \
             KEPT (Changed), not omitted; present = {present_tick3:?}"
        );
        let state = frame3
            .iter()
            .find(|r| r.key.as_str() == key)
            .map(|r| r.state);
        assert_eq!(
            state,
            Some(WireProjectionState::Changed),
            "Tick 3 (mixed): kept key `{key}` must carry state=Changed; got {state:?}"
        );
    }

    // Omitted (Unchanged): every other non-drain Tier-2 key. Their inputs did
    // not move this tick, so the omission layer must drop them from the SAME
    // frame that still carries the changed cluster.
    let changed_cluster: std::collections::HashSet<&str> = [
        "configured_relays",
        "relay_role_options",
        "settings_hub",
        "relay_diagnostics",
    ]
    .into_iter()
    .collect();
    for &key in tier2_keys() {
        if changed_cluster.contains(key) {
            continue;
        }
        // Drain/stage keys are conditional (absent without settlements) — their
        // absence is not an omission signal, so skip them here.
        if matches!(key, "action_results" | "signed_events" | "action_stages" | "action_lifecycle") {
            continue;
        }
        assert!(
            !present_tick3.contains(key),
            "Tick 3 (mixed): key `{key}` was NOT affected by set_configured_relays → \
             must be ABSENT (Unchanged → omitted) while the changed cluster is kept; \
             present = {present_tick3:?}"
        );
    }
}
