//! Cardinal-trap tests for [`FeedEmissionState`] — ADR-0055 Rung 6 S1.
//!
//! Group A: every visible-output mutation must produce different bytes (emit +
//! rev bump). Group B: non-output mutations produce identical bytes (omit, rev
//! stable). Group C: host-coherence simulation across emit/omit/identity-reset
//! sequences; reconstructed host feed always matches the full payload. Group C
//! also covers the R6-S1 freeze fix: a host cache reset (session_id OR
//! snapshot_epoch change) while the producer's state is preserved MUST force a
//! producer baseline, not an omit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{FeedEmissionState, FrameIdentity};

// ── Host-coherence simulation helper ─────────────────────────────────────────

/// Simulates the host `ProjectionCache` omit==retain + reset semantics, used by
/// Group C tests to verify the reconstructed feed equals the full-emit feed.
///
/// Mirrors `ProjectionCache.generated.swift`: a `Changed` frame overwrites; an
/// omit (absent key) retains the prior value; a frame whose `(session_id,
/// snapshot_epoch)` differs from the cached identity triggers `removeAll()`
/// BEFORE the frame is applied — the exact two-axis reset the freeze fix keys on.
struct HostCacheSim {
    cached: Option<Vec<u8>>,
    cached_rev: u64,
    identity: Option<FrameIdentity>,
}

impl HostCacheSim {
    fn new() -> Self {
        Self {
            cached: None,
            cached_rev: 0,
            identity: None,
        }
    }

    /// Apply a frame tick.
    ///
    /// * `result` — the `should_emit` return value: `Some((payload, rev))` for
    ///   an emit frame, `None` for an omit frame.
    /// * `identity` — this frame's `(session_id, snapshot_epoch)`. A change from
    ///   the cached identity resets the host cache (removeAll) before applying.
    /// * `full_payload` — the true full payload for this tick (what the host
    ///   SHOULD have after this frame, regardless of omit/emit).
    ///
    /// Asserts the host-coherence invariant: reconstructed cache == full_payload.
    fn apply(
        &mut self,
        result: Option<(Vec<u8>, u64)>,
        identity: FrameIdentity,
        full_payload: &[u8],
    ) {
        // Two-axis reset: session_id OR snapshot_epoch change → removeAll().
        if self.identity != Some(identity) {
            self.identity = Some(identity);
            self.cached = None;
            self.cached_rev = 0;
        }

        match result {
            Some((payload, incoming_rev)) => {
                // Reorder guard: host drops frames with rev <= cached rev.
                if incoming_rev > self.cached_rev {
                    self.cached = Some(payload);
                    self.cached_rev = incoming_rev;
                }
            }
            None => {
                // Omit frame → host retains prior value (omit==retain invariant).
            }
        }

        assert_eq!(
            self.cached.as_deref().unwrap_or(&[]),
            full_payload,
            "host-coherence invariant violated: reconstructed feed \
             does not match the full-emit payload"
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn capability_on() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(true))
}

fn capability_off() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
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

// ── Group C: host-coherence simulation ───────────────────────────────────────

/// C.1 — Omit frame: host retains prior value (omit==retain invariant).
#[test]
fn c1_omit_retains_prior_value() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p = payload(0xDE, 58_768);
    host.apply(state.should_emit(p.clone(), id0()), id0(), &p);
    let r2 = state.should_emit(p.clone(), id0());
    assert!(r2.is_none());
    host.apply(r2, id0(), &p);
}

/// C.2 — Changed frame after omit: host overwrites with new value.
#[test]
fn c2_changed_after_omit_overwrites_host_cache() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p1 = payload(0x11, 58_768);
    let p2 = payload(0x22, 58_768);
    host.apply(state.should_emit(p1.clone(), id0()), id0(), &p1);
    host.apply(state.should_emit(p1.clone(), id0()), id0(), &p1);
    let r3 = state.should_emit(p2.clone(), id0());
    assert!(r3.is_some(), "changed payload must emit");
    host.apply(r3, id0(), &p2);
}

/// C.3 — Account-switch epoch change → host resets cache, then baseline.
#[test]
fn c3_epoch_change_resets_host_cache_then_baseline() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let pa = payload(0xAA, 58_768);
    let pb = payload(0xBB, 58_768);
    host.apply(state.should_emit(pa.clone(), id0()), id0(), &pa);
    let idle = state.should_emit(pa.clone(), id0());
    assert!(idle.is_none());
    host.apply(idle, id0(), &pa);

    let switched = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 1,
    };
    let r3 = state.should_emit(pb.clone(), switched);
    assert!(
        r3.is_some(),
        "first tick after epoch change must be a baseline"
    );
    assert_eq!(
        r3.as_ref().unwrap().1,
        1,
        "rev resets to 1 after epoch change"
    );
    host.apply(r3, switched, &pb);
}

/// C.4 — Capability OFF: every tick emits (byte-identical to today).
#[test]
fn c4_capability_off_always_emits() {
    let mut state = FeedEmissionState::new(capability_off());
    let p = payload(0xAA, 58_768);
    for i in 1..=40 {
        assert!(
            state.should_emit(p.clone(), id0()).is_some(),
            "capability OFF must always emit (tick {i})"
        );
    }
}

/// C.5 — Monotonic rev keeps the host reorder guard correct.
#[test]
fn c5_monotonic_rev_keeps_reorder_guard_correct() {
    let mut state = FeedEmissionState::new(capability_on());
    let rev1 = state.should_emit(payload(0x10, 58_768), id0()).unwrap().1;
    let rev2 = state.should_emit(payload(0x20, 58_768), id0()).unwrap().1;
    assert!(rev2 > rev1, "rev must be strictly monotonically increasing");
}

/// C.6 — Full host-coherence sequence: 5 unique payloads, each repeated 3x.
#[test]
fn c6_host_coherence_full_sequence() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let payloads: Vec<Vec<u8>> = (0u8..5)
        .flat_map(|tag| std::iter::repeat(payload(tag, 58_768 + tag as usize)).take(3))
        .collect();
    for p in payloads.iter() {
        host.apply(state.should_emit(p.clone(), id0()), id0(), p);
    }
}

/// C.7 — Capability OFF is byte-identical to today: idle ticks still emit with
/// monotonically increasing revs.
#[test]
fn c7_capability_off_is_byte_identical_to_today() {
    let mut state = FeedEmissionState::new(capability_off());
    let p = payload(0xFF, 58_768);
    let r1 = state.should_emit(p.clone(), id0()).expect("emit tick 1");
    let r2 = state
        .should_emit(p.clone(), id0())
        .expect("emit tick 2 (idle)");
    let r3 = state
        .should_emit(p.clone(), id0())
        .expect("emit tick 3 (idle)");
    assert_eq!(r1.0, p);
    assert_eq!(r2.0, p);
    assert_eq!(r3.0, p);
    assert!(r2.1 > r1.1 && r3.1 > r2.1, "revs monotonically increasing");
}

/// C.8 — 100-tick sequence with changes every 10 ticks.
#[test]
fn c8_long_sequence_host_coherence() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let mut current = payload(0x00, 58_768);
    for tick in 0u64..100 {
        if tick % 10 == 0 {
            current = payload((tick % 256) as u8, 58_768 + (tick as usize % 100));
        }
        host.apply(state.should_emit(current.clone(), id0()), id0(), &current);
    }
}

/// C.9 — Capability flag propagates from a shared AtomicBool (the production
/// wiring pattern: main thread sets it, actor thread reads it in the closure).
#[test]
fn c9_capability_flag_propagates_from_shared_atomic() {
    let flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let mut state = FeedEmissionState::new(Arc::clone(&flag));
    let p = payload(0xAB, 58_768);
    assert!(
        state.should_emit(p.clone(), id0()).is_some(),
        "cap OFF emit 1"
    );
    assert!(
        state.should_emit(p.clone(), id0()).is_some(),
        "cap OFF emit 2"
    );
    flag.store(true, Ordering::Release); // declare_incremental_apply
    assert!(
        state.should_emit(p.clone(), id0()).is_none(),
        "cap ON after flag store → idle tick omits"
    );
}

/// C.10 — THE FREEZE TEST (R6-S1 kill criterion). `ActorCommand::Reset` rebuilds
/// the kernel → new `session_id`, but the producer's `FeedEmissionState` and the
/// engine `Arc` SURVIVE, so the next tick encodes BYTE-IDENTICAL bytes. The host
/// cache reset (new session_id → removeAll) means an omit here would leave the
/// host with NO feed entry → frozen/blank timeline.
///
/// Against the pre-fix code (single `epoch` param, blind to `session_id`) the
/// producer would OMIT and this test would FAIL (host reconstructs empty != p).
/// With the fix (identity = `(session_id, snapshot_epoch)`) the changed
/// `session_id` forces a baseline, so the producer EMITS and the host stays
/// coherent.
#[test]
fn c10_reset_new_session_id_forces_baseline_not_omit() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p = payload(0xC0, 58_768);

    // Pre-Reset session.
    let pre = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    };
    host.apply(state.should_emit(p.clone(), pre), pre, &p);
    // Idle tick → omit, host retains.
    let idle = state.should_emit(p.clone(), pre);
    assert!(idle.is_none(), "pre-Reset idle tick omits");
    host.apply(idle, pre, &p);

    // ── Reset: kernel rebuild → new started_unix_ms → new session_id. The
    // producer state + engine survive, so the SAME bytes `p` are encoded. ──
    let post = FrameIdentity {
        session_id: 2_000, // changed: new kernel run
        snapshot_epoch: 0, // fresh kernel resets epoch to 0
    };
    let post_reset = state.should_emit(p.clone(), post);
    assert!(
        post_reset.is_some(),
        "FREEZE GUARD: byte-identical payload after a session_id change MUST \
         emit a baseline (host cache was reset), never omit"
    );
    assert_eq!(
        post_reset.as_ref().unwrap().1,
        1,
        "rev restarts at 1 post-Reset"
    );
    // Host applies under the new identity (its cache was reset) and stays coherent.
    host.apply(post_reset, post, &p);
}

/// C.11 — Freeze guard, epoch axis: a `snapshot_epoch` change with byte-identical
/// payload (account switch where the new account's feed encodes identically —
/// e.g. both empty) MUST emit a baseline, not omit.
#[test]
fn c11_epoch_change_identical_bytes_forces_baseline_not_omit() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let empty_feed = payload(0x00, 200); // both accounts' empty feed encode the same

    let a = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    };
    host.apply(state.should_emit(empty_feed.clone(), a), a, &empty_feed);

    // Account switch: epoch bumps, but the new (empty) account's feed encodes
    // to the IDENTICAL bytes. Host reset its cache on the epoch change.
    let b = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 1,
    };
    let switched = state.should_emit(empty_feed.clone(), b);
    assert!(
        switched.is_some(),
        "FREEZE GUARD: identical bytes after an epoch change MUST emit a baseline"
    );
    host.apply(switched, b, &empty_feed);
}
