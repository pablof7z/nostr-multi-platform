//! Cardinal-trap tests for [`FeedEmissionState`] — ADR-0055 Rung 6 S1.
//!
//! Group A: every visible-output mutation must produce different bytes (emit +
//! rev bump). Group B: non-output mutations produce identical bytes (omit, rev
//! stable). Host-coherence/freeze-guard cases live in
//! `emission_state_host_tests.rs`.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::{FeedEmissionState, FrameIdentity};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn capability_on() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(true))
}

fn payload(tag: u8, size: usize) -> Vec<u8> {
    vec![tag; size]
}

/// A stable frame identity for the steady-state (no Reset / no account switch).
fn id0() -> FrameIdentity {
    FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    }
}

// ── Group A: every visible-output mutation MUST emit ─────────────────────────

/// A.1 — First emission after construction is always a full emit.
#[test]
fn a1_first_emission_is_always_full_emit() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0xAA, 58_768);
    let result = state.should_emit(p.clone(), id0());
    assert!(result.is_some(), "first emission must never be omitted");
    let (emitted_payload, rev) = result.unwrap();
    assert_eq!(
        emitted_payload, p,
        "first emission must carry the full payload"
    );
    assert_eq!(rev, 1, "first emission rev must be 1");
    assert_eq!(state.current_rev(), 1);
}

/// A.2 — Changed payload (new in-window root / card content edit) → emit.
#[test]
fn a2_changed_payload_emits_and_bumps_rev() {
    let mut state = FeedEmissionState::new(capability_on());
    state.should_emit(payload(0x01, 58_768), id0());
    let result = state.should_emit(payload(0x02, 58_768), id0());
    assert!(result.is_some(), "changed payload must emit");
    assert_eq!(result.unwrap().1, 2, "rev must advance 1 -> 2");
}

/// A.3 — Card removal (payload shrinks / content changes) → emit.
#[test]
fn a3_card_removal_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    state.should_emit(payload(0xFF, 58_872), id0());
    let result = state.should_emit(payload(0xFE, 57_500), id0());
    assert!(result.is_some(), "card removal must emit");
    assert_eq!(result.unwrap().1, 2);
}

/// A.4 — Card reorder (bytes change even if same cards) → emit.
#[test]
fn a4_card_reorder_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut ordered = vec![0u8; 200];
    ordered[0] = 0xAA;
    ordered[100] = 0xBB;
    let mut reordered = vec![0u8; 200];
    reordered[0] = 0xBB;
    reordered[100] = 0xAA;
    state.should_emit(ordered, id0());
    assert!(
        state.should_emit(reordered, id0()).is_some(),
        "reorder must emit (bytes differ)"
    );
}

/// A.5 — Attribution added to a visible root → payload changes → emit.
#[test]
fn a5_attribution_added_to_visible_root_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    state.should_emit(payload(0x10, 58_000), id0());
    assert!(
        state.should_emit(payload(0x10, 58_736), id0()).is_some(),
        "attribution add must emit"
    );
}

/// A.6 — Attribution removed from a visible root → payload changes → emit.
#[test]
fn a6_attribution_removed_from_visible_root_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    state.should_emit(payload(0x10, 58_736), id0());
    assert!(
        state.should_emit(payload(0x10, 58_000), id0()).is_some(),
        "attribution remove must emit"
    );
}

/// A.7 — Any visible author-display byte change → emit. This is a structural
/// equality guard, not a feed-owned profile-refresh path.
#[test]
fn a7_visible_author_display_change_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    state.should_emit(b"...author:Alice...".to_vec(), id0());
    let result = state.should_emit(b"...author:Alice Renamed...".to_vec(), id0());
    assert!(result.is_some(), "visible card byte change must emit");
    assert_eq!(result.unwrap().1, 2);
}

/// A.8 — Window-content change: the typed sidecar always snapshots the default
/// `FeedRequest`, so the window size is fixed. What changes is the SET of roots
/// that fall in that fixed window as new in-window roots arrive — the encoded
/// bytes grow. Representative of the real sidecar path (no `load_older`).
#[test]
fn a8_window_content_growth_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    state.should_emit(payload(0x55, 40_000), id0()); // partial window
    let result = state.should_emit(payload(0x55, 58_872), id0()); // window fills
    assert!(result.is_some(), "window content growth must emit");
    assert_eq!(result.unwrap().1, 2);
}

/// A.9 — Account switch → snapshot_epoch bumps → forced full baseline re-emit
/// with rev reset to 1.
#[test]
fn a9_account_switch_epoch_change_forces_baseline() {
    let mut state = FeedEmissionState::new(capability_on());
    state.should_emit(payload(0xAA, 58_768), id0()); // epoch 0, rev 1

    // Account switch: snapshot_epoch 0 -> 1 (same session).
    let switched = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 1,
    };
    let p2 = payload(0xBB, 58_768);
    let result = state.should_emit(p2.clone(), switched);
    assert!(
        result.is_some(),
        "epoch change must force a baseline re-emit"
    );
    let (emitted, rev) = result.unwrap();
    assert_eq!(emitted, p2);
    assert_eq!(rev, 1, "rev must reset to 1 after epoch change");
    assert_eq!(state.current_identity(), Some(switched));
}

/// A.10 — New in-window root: bytes differ because a new root card was
/// appended → emit.
#[test]
fn a10_new_in_window_root_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    state.should_emit(payload(0x00, 200), id0()); // empty feed
    let result = state.should_emit(payload(0x01, 58_736), id0()); // one card
    assert!(result.is_some(), "new in-window root must emit");
    assert_eq!(result.unwrap().1, 2);
}

// ── Group B: non-output mutations MUST omit ───────────────────────────────────

/// B.1 — Idle tick (no ingest, payload byte-identical) → omit.
#[test]
fn b1_idle_tick_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0xAA, 58_768);
    state.should_emit(p.clone(), id0());
    assert!(state.should_emit(p, id0()).is_none(), "idle tick must omit");
    assert_eq!(state.current_rev(), 1, "rev must not advance on omit");
}

/// B.2 — Multiple consecutive idle ticks → all omitted, rev stable.
#[test]
fn b2_multiple_idle_ticks_all_omit() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0xCC, 58_768);
    state.should_emit(p.clone(), id0());
    for _ in 0..39 {
        assert!(
            state.should_emit(p.clone(), id0()).is_none(),
            "idle must omit"
        );
    }
    assert_eq!(state.current_rev(), 1, "rev stable across 39 idle ticks");
}

/// B.3 — Out-of-window event: the engine's `snapshot()` only materializes the
/// visible window, so encoded bytes are unchanged → omit.
#[test]
fn b3_out_of_window_event_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0x77, 58_872);
    state.should_emit(p.clone(), id0());
    assert!(
        state.should_emit(p, id0()).is_none(),
        "out-of-window must omit"
    );
}

/// B.4 — Duplicate event (already held): engine state unchanged → omit.
#[test]
fn b4_duplicate_event_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0x44, 58_768);
    state.should_emit(p.clone(), id0());
    assert!(state.should_emit(p, id0()).is_none(), "duplicate must omit");
}

/// B.5 — Attribution added to a non-visible root → visible bytes unchanged →
/// omit.
#[test]
fn b5_attribution_on_non_visible_root_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0x33, 58_872);
    state.should_emit(p.clone(), id0());
    assert!(
        state.should_emit(p, id0()).is_none(),
        "non-visible attr must omit"
    );
}

/// B.6 — Non-visible author metadata change → visible bytes unchanged → omit.
#[test]
fn b6_non_visible_author_metadata_change_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0x22, 58_000);
    state.should_emit(p.clone(), id0());
    assert!(
        state.should_emit(p, id0()).is_none(),
        "non-visible refresh omit"
    );
}
