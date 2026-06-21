//! ADR-0055 R6-S2 per-key cardinal-trap tests for the `nip46_onboarding` typed
//! projection emission gate.
//!
//! The generic omit logic is exercised exhaustively in
//! `nmp_core::projection_emission_tests`; these tests confirm the INTEGRATION
//! path for THIS key — the real `nip46_onboarding_typed` producer driven through
//! a `TypedProjectionEmissionState` exactly as `crate::actor` wires it. They
//! mirror `nmp-ffi`'s `embed_sidecar_emission_tests` so both whole-value Tier-1
//! keys carry symmetric per-key freeze guards.
//!
//! Group A: value changes → emit. Group B: value unchanged → omit.
//! Group C: freeze guard — session_id / epoch change with identical bytes MUST
//!   emit a baseline (the cardinal regression: an omit there would leave the
//!   host's reset `ProjectionCache` with no `nip46_onboarding` entry → frozen UI).
//! Group D: capability OFF → always emit.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::nip46_onboarding_typed;
use crate::actor::commands::{new_bunker_handshake_slot, BunkerHandshakeDto, BunkerHandshakeSlot};
use crate::projection_emission::{FrameIdentity, TypedProjectionEmissionState};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn capability_on() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(true))
}

fn capability_off() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn set_stage(slot: &BunkerHandshakeSlot, stage: &str, message: Option<&str>) {
    *slot.lock().unwrap() = Some(BunkerHandshakeDto::new(
        stage.to_string(),
        None,
        message.map(str::to_string),
    ));
}

/// Simulate one producer tick: build the typed payload via the REAL
/// `nip46_onboarding_typed` builder, then run it through the emission state —
/// the exact sequence `crate::actor`'s registered closure performs.
///
/// `nip46_onboarding` is an always-present key (the builder never returns
/// `None`), so the `?`-style early return in the production closure never fires
/// here; we unwrap to keep the trap surface honest.
fn tick(
    slot: &BunkerHandshakeSlot,
    state: &Arc<Mutex<TypedProjectionEmissionState>>,
    frame_session_id: &Arc<AtomicU64>,
    frame_snapshot_epoch: &Arc<AtomicU64>,
) -> Option<(Vec<u8>, u64)> {
    let typed_data = nip46_onboarding_typed(slot).expect("nip46_onboarding is always present");
    let identity = FrameIdentity {
        session_id: frame_session_id.load(Ordering::Acquire),
        snapshot_epoch: frame_snapshot_epoch.load(Ordering::Acquire),
    };
    let mut st = state.lock().expect("emission state lock");
    let decision = st.should_emit(typed_data.payload, identity);
    drop(st);
    decision
}

// ── Group A: value changes → emit ─────────────────────────────────────────────

/// A.1 — First tick always emits a baseline (even from the idle/empty slot).
#[test]
fn nip46_onboarding_a1_first_tick_always_emits() {
    let slot = new_bunker_handshake_slot(); // idle
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(capability_on())));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    let result = tick(&slot, &state, &sid, &epoch);
    assert!(result.is_some(), "first tick must always emit a baseline");
    assert_eq!(result.unwrap().1, 1, "first emission rev must be 1");
}

/// A.2 — Handshake stage transition changes the payload bytes → emit.
#[test]
fn nip46_onboarding_a2_stage_transition_emits() {
    let slot = new_bunker_handshake_slot();
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(capability_on())));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    tick(&slot, &state, &sid, &epoch); // idle baseline
    set_stage(&slot, "connecting", Some("wss://relay.example"));

    let result = tick(&slot, &state, &sid, &epoch);
    assert!(result.is_some(), "stage transition must emit");
    assert_eq!(result.unwrap().1, 2, "rev must advance to 2");
}

/// A.3 — Returning to idle (handshake cleared) changes the bytes back → emit.
#[test]
fn nip46_onboarding_a3_return_to_idle_emits() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "connecting", None);
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(capability_on())));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    tick(&slot, &state, &sid, &epoch); // connecting baseline
    *slot.lock().unwrap() = None; // back to idle

    let result = tick(&slot, &state, &sid, &epoch);
    assert!(result.is_some(), "return to idle changes bytes → must emit");
}

// ── Group B: value unchanged → omit ───────────────────────────────────────────

/// B.1 — Idle tick (slot unchanged) → omit, rev stable.
#[test]
fn nip46_onboarding_b1_idle_tick_omits() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "connecting", Some("wss://relay.example"));
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(capability_on())));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    tick(&slot, &state, &sid, &epoch); // baseline
    let idle = tick(&slot, &state, &sid, &epoch);
    assert!(idle.is_none(), "idle tick (slot unchanged) must omit");
    assert_eq!(
        state.lock().unwrap().current_rev(),
        1,
        "rev must not advance on omit"
    );
}

/// B.2 — Multiple consecutive idle ticks → all omitted, rev stable.
#[test]
fn nip46_onboarding_b2_multiple_idle_ticks_omit() {
    let slot = new_bunker_handshake_slot(); // idle, stable
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(capability_on())));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    tick(&slot, &state, &sid, &epoch); // baseline
    for i in 0..19 {
        assert!(
            tick(&slot, &state, &sid, &epoch).is_none(),
            "idle tick {i} must omit"
        );
    }
    assert_eq!(
        state.lock().unwrap().current_rev(),
        1,
        "rev stable across 19 idle ticks"
    );
}

// ── Group C: freeze guard ─────────────────────────────────────────────────────

/// C.1 — THE FREEZE TEST for `nip46_onboarding`. `ActorCommand::Reset` rebuilds
/// the kernel → new `session_id`, but the producer emission state SURVIVES. The
/// slot content is unchanged so the typed payload is BYTE-IDENTICAL. The host
/// cache reset (new session_id → removeAll) means an omit here would leave the
/// host with NO `nip46_onboarding` entry → a frozen onboarding sheet.
///
/// Against a naive impl (byte-equality only, no identity check) the producer
/// would OMIT and this test would FAIL. With the R6-S2 fix the changed
/// `session_id` forces a baseline → EMITS at rev 1.
#[test]
fn nip46_onboarding_c1_freeze_guard_session_id_change_forces_baseline() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "connecting", Some("wss://relay.example"));
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(capability_on())));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    // Pre-Reset session baseline + an idle tick that omits.
    tick(&slot, &state, &sid, &epoch);
    assert!(
        tick(&slot, &state, &sid, &epoch).is_none(),
        "pre-Reset idle omits"
    );

    // Reset: kernel rebuild → new session_id. Slot content unchanged.
    sid.store(2_000, Ordering::Release);

    let post_reset = tick(&slot, &state, &sid, &epoch);
    assert!(
        post_reset.is_some(),
        "FREEZE GUARD: byte-identical payload after session_id change MUST emit \
         a baseline (host cache was reset), never omit"
    );
    assert_eq!(post_reset.unwrap().1, 1, "rev restarts at 1 post-Reset");
}

/// C.2 — Freeze guard, epoch axis: `snapshot_epoch` change with identical bytes
/// (account switch where the new account is also idle → the always-present
/// onboarding projection encodes the SAME idle bytes) MUST emit a baseline.
#[test]
fn nip46_onboarding_c2_freeze_guard_epoch_change_identical_bytes_forces_baseline() {
    let slot = new_bunker_handshake_slot(); // idle
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(capability_on())));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    tick(&slot, &state, &sid, &epoch); // idle baseline
    assert!(
        tick(&slot, &state, &sid, &epoch).is_none(),
        "idle omits"
    );

    // Account switch: epoch bumps; the new account is also idle (same bytes).
    epoch.store(1, Ordering::Release);

    let switched = tick(&slot, &state, &sid, &epoch);
    assert!(
        switched.is_some(),
        "FREEZE GUARD: identical idle bytes after epoch change MUST emit a baseline"
    );
}

// ── Group D: capability-OFF ───────────────────────────────────────────────────

/// D.1 — Capability OFF: every tick emits (byte-identical to pre-R6-S2 behavior).
#[test]
fn nip46_onboarding_d1_capability_off_always_emits() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "connecting", Some("wss://relay.example"));
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(capability_off())));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    for i in 1..=10 {
        assert!(
            tick(&slot, &state, &sid, &epoch).is_some(),
            "capability OFF must always emit (tick {i})"
        );
    }
}
