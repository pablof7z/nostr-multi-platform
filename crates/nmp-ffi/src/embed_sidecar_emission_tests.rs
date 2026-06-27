//! ADR-0055 R6-S2 cardinal-trap tests for the `claimed_event_embeds` typed
//! projection emission gate.
//!
//! Tests prove the `TypedProjectionEmissionState` wrapping in
//! `install_embed_sidecar_projection` behaves correctly for this whole-value
//! key. The omit logic itself is tested exhaustively in
//! `nmp_core::projection_emission_tests`; these tests confirm the INTEGRATION
//! path (actual producer output for `claimed_event_embeds`).
//!
//! ## Groups (parallel to R6-S1 Group A/B/C)
//!
//! Group A: value changes → emit (trap-proof: any byte change emits).
//! Group B: value unchanged → omit, rev stable.
//! Group C: freeze guard — session_id and epoch changes with identical bytes
//!   MUST emit a baseline (the cardinal freeze-class regression guard).
//! Group D: capability-OFF → always emit (byte-identical to today).
//! Group E: host-coherence simulation (omit==retain reconstructs correctly).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_content::{
    EmbedKindProjection, EmbeddedEventEnvelope, RenderContextWire, UnknownProjection,
};
use nmp_core::projection_emission::{FrameIdentity, TypedProjectionEmissionState};

use super::{new_embed_sidecar_slot, read_embed_sidecar_typed, EmbedSidecarSlot};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn capability_on() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(true))
}

fn capability_off() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn id0() -> FrameIdentity {
    FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    }
}

/// Simulate one producer tick: build payload from slot, run through emission state.
fn tick(
    slot: &EmbedSidecarSlot,
    state: &Arc<Mutex<TypedProjectionEmissionState>>,
    frame_session_id: &Arc<AtomicU64>,
    frame_snapshot_epoch: &Arc<AtomicU64>,
) -> Option<(Vec<u8>, u64)> {
    let typed_data = read_embed_sidecar_typed(slot);
    let identity = FrameIdentity {
        session_id: frame_session_id.load(Ordering::Acquire),
        snapshot_epoch: frame_snapshot_epoch.load(Ordering::Acquire),
    };
    let Ok(mut st) = state.lock() else {
        return None;
    };
    let decision = st.should_emit(typed_data.payload.clone(), identity);
    drop(st);
    decision
}

/// Build a non-empty BTreeMap entry by reusing the embed JSON encode path via
/// `read_embed_sidecar_typed` — we just verify the payload CHANGES when the slot
/// changes, so any non-empty slot content is sufficient.
fn populate_slot_with_marker(slot: &EmbedSidecarSlot, marker: u8) {
    // Uses module-level imports: EmbeddedEventEnvelope, EmbedKindProjection,
    // RenderContextWire, UnknownProjection, ContentTreeWire.
    let mut map: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
    let key = format!("marker_{marker}");
    map.insert(
        key.clone(),
        EmbeddedEventEnvelope {
            uri: String::new(),
            primary_id: key,
            render_context: RenderContextWire {
                depth: 0,
                max_depth: 4,
                visited: vec![],
            },
            projection: EmbedKindProjection::Unknown(UnknownProjection {
                kind: marker as u32,
                author_pubkey: "aa".repeat(32),
                author_display_name: None,
                author_picture_url: None,
                created_at: 1_000_000,
                content: format!("content_{marker}"),
                content_tree: nmp_content::wire::ContentTreeWire::default(),
                tags: vec![],
                alt_text: None,
            }),
            collapsed: false,
            collapse_reason: None,
        },
    );
    slot.lock().unwrap().envelopes = map;
}

// ── Group A: value changes → emit ────────────────────────────────────────────

/// A.1 — First tick always emits a full baseline.
#[test]
fn claimed_event_embeds_a1_first_tick_always_emits() {
    let slot = new_embed_sidecar_slot(); // empty
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_on(),
    )));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    let result = tick(&slot, &state, &sid, &epoch);
    assert!(result.is_some(), "first tick must always emit a baseline");
    assert_eq!(result.unwrap().1, 1, "first emission rev must be 1");
}

/// A.2 — Slot gains a new embed → bytes change → emit.
#[test]
fn claimed_event_embeds_a2_new_embed_emits() {
    let slot = new_embed_sidecar_slot(); // empty
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_on(),
    )));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    // First tick — empty slot.
    tick(&slot, &state, &sid, &epoch);

    // Add a new embed.
    populate_slot_with_marker(&slot, 1);

    let result = tick(&slot, &state, &sid, &epoch);
    assert!(result.is_some(), "new embed must emit");
    assert_eq!(result.unwrap().1, 2, "rev must advance to 2");
}

/// A.3 — Slot cleared → bytes change (non-empty NEMB → empty NEMB) → emit.
#[test]
fn claimed_event_embeds_a3_slot_cleared_emits() {
    let slot = new_embed_sidecar_slot();
    populate_slot_with_marker(&slot, 7);
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_on(),
    )));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    tick(&slot, &state, &sid, &epoch); // baseline

    slot.lock().unwrap().envelopes = BTreeMap::new(); // clear

    let result = tick(&slot, &state, &sid, &epoch);
    assert!(
        result.is_some(),
        "clearing the slot must emit (bytes change)"
    );
}

// ── Group B: value unchanged → omit ──────────────────────────────────────────

/// B.1 — Idle tick (slot unchanged) → omit.
#[test]
fn claimed_event_embeds_b1_idle_tick_omits() {
    let slot = new_embed_sidecar_slot();
    populate_slot_with_marker(&slot, 3);
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_on(),
    )));
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
fn claimed_event_embeds_b2_multiple_idle_ticks_omit() {
    let slot = new_embed_sidecar_slot();
    populate_slot_with_marker(&slot, 5);
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_on(),
    )));
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

/// C.1 — THE FREEZE TEST for `claimed_event_embeds`. `ActorCommand::Lifecycle(LifecycleCommand::Reset)`
/// rebuilds the kernel → new `session_id`, but the producer emission state
/// SURVIVES. The slot content may encode to BYTE-IDENTICAL bytes. The host
/// cache reset (new session_id → removeAll) means an omit here would leave the
/// host with NO `claimed_event_embeds` entry → frozen, blank embeds.
///
/// Against a naive impl (byte-equality only, no identity check), the producer
/// would OMIT and this test would FAIL.
/// With R6-S2 fix, the changed `session_id` forces a baseline → EMITS.
#[test]
fn claimed_event_embeds_c1_freeze_guard_session_id_change_forces_baseline() {
    let slot = new_embed_sidecar_slot();
    populate_slot_with_marker(&slot, 9);
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_on(),
    )));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    // Pre-Reset session baseline.
    tick(&slot, &state, &sid, &epoch);
    // Idle tick → omit, host retains.
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
    let (_, rev) = post_reset.unwrap();
    assert_eq!(rev, 1, "rev restarts at 1 post-Reset");
}

/// C.2 — Freeze guard, epoch axis: `snapshot_epoch` change with identical bytes
/// (account switch where the new account's `claimed_event_embeds` encodes the
/// same — e.g. both empty maps) MUST emit a baseline, not omit.
#[test]
fn claimed_event_embeds_c2_freeze_guard_epoch_change_identical_bytes_forces_baseline() {
    let slot = new_embed_sidecar_slot(); // empty slot
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_on(),
    )));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    tick(&slot, &state, &sid, &epoch); // baseline (empty NEMB)
    assert!(tick(&slot, &state, &sid, &epoch).is_none(), "idle omits");

    // Account switch: epoch bumps, but the new account also has empty embeds.
    epoch.store(1, Ordering::Release);

    let switched = tick(&slot, &state, &sid, &epoch);
    assert!(
        switched.is_some(),
        "FREEZE GUARD: identical bytes after epoch change MUST emit a baseline"
    );
}

// ── Group D: capability-OFF ───────────────────────────────────────────────────

/// D.1 — Capability OFF: every tick emits (byte-identical to today).
#[test]
fn claimed_event_embeds_d1_capability_off_always_emits() {
    let slot = new_embed_sidecar_slot();
    populate_slot_with_marker(&slot, 2);
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_off(),
    )));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    for i in 1..=10 {
        assert!(
            tick(&slot, &state, &sid, &epoch).is_some(),
            "capability OFF must always emit (tick {i})"
        );
    }
}

// ── Group E: host-coherence simulation ────────────────────────────────────────

/// E.1 — Full host-coherence sequence: slot changes, then idle, then Reset.
/// Reconstructed host cache always equals the full payload.
#[test]
fn claimed_event_embeds_e1_host_coherence_full_sequence() {
    let slot = new_embed_sidecar_slot();
    let state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        capability_on(),
    )));
    let sid = Arc::new(AtomicU64::new(1_000));
    let epoch = Arc::new(AtomicU64::new(0));

    // Helper: simulate host ProjectionCache apply.
    let mut host_cache: Option<Vec<u8>> = None;
    let mut host_rev: u64 = 0;
    let mut host_identity: Option<FrameIdentity> = None;

    macro_rules! apply_host {
        ($result:expr, $identity:expr) => {{
            let full_payload = read_embed_sidecar_typed(&slot).payload;
            let identity: FrameIdentity = $identity;
            if host_identity != Some(identity) {
                host_identity = Some(identity);
                host_cache = None;
                host_rev = 0;
            }
            if let Some((payload, incoming_rev)) = $result {
                if incoming_rev > host_rev {
                    host_cache = Some(payload);
                    host_rev = incoming_rev;
                }
            }
            assert_eq!(
                host_cache.as_deref().unwrap_or(&[]),
                full_payload.as_slice(),
                "host-coherence invariant violated"
            );
        }};
    }

    let id_a = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    };

    // Tick 1: empty slot baseline.
    apply_host!(tick(&slot, &state, &sid, &epoch), id_a);

    // Tick 2: idle — omit.
    let r2 = tick(&slot, &state, &sid, &epoch);
    assert!(r2.is_none(), "idle must omit");
    apply_host!(r2, id_a);

    // Tick 3: slot gains embed.
    populate_slot_with_marker(&slot, 42);
    apply_host!(tick(&slot, &state, &sid, &epoch), id_a);

    // Tick 4: Reset — new session_id.
    sid.store(2_000, Ordering::Release);
    let id_b = FrameIdentity {
        session_id: 2_000,
        snapshot_epoch: 0,
    };
    let r4 = tick(&slot, &state, &sid, &epoch);
    assert!(r4.is_some(), "post-Reset baseline must emit");
    apply_host!(r4, id_b);

    // Tick 5: idle under new session.
    let r5 = tick(&slot, &state, &sid, &epoch);
    assert!(r5.is_none(), "idle under new session must omit");
    apply_host!(r5, id_b);
}
