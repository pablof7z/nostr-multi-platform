//! Cardinal-trap tests for [`FeedEmissionState`] — ADR-0055 Rung 6 S1.
//!
//! Group A: every visible-output mutation must produce different bytes (emit +
//! rev bump). Group B: non-output mutations produce identical bytes (omit, rev
//! stable). Group C: host-coherence simulation across emit/omit/epoch-reset
//! sequences; reconstructed host feed always matches the full payload.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::FeedEmissionState;

// ── Host-coherence simulation helper ─────────────────────────────────────────

/// Simulates the `ProjectionMergeCache` omit==retain semantics host-side,
/// used by Group C tests to verify the reconstructed feed equals the
/// full-emit feed across a sequence of omit/emit/epoch-reset frames.
///
/// The host cache: `Changed` frame → overwrite; omit (absent key) → retain
/// prior; epoch change → full reset then accept the baseline.
struct HostCacheSim {
    cached: Option<Vec<u8>>,
    cached_rev: u64,
    epoch: u64,
}

impl HostCacheSim {
    fn new() -> Self {
        Self {
            cached: None,
            cached_rev: 0,
            epoch: 0,
        }
    }

    /// Apply a frame tick.
    ///
    /// * `result` — the `should_emit` return value: `Some((payload, rev))` for
    ///   an emit frame, `None` for an omit frame.
    /// * `epoch` — the current frame epoch (signals epoch change to the host).
    /// * `full_payload` — the true full payload for this tick (what the host
    ///   SHOULD have after this frame, regardless of omit/emit).
    ///
    /// Asserts the omit==retain invariant: reconstructed cache == full_payload.
    fn apply(&mut self, result: Option<(Vec<u8>, u64)>, epoch: u64, full_payload: &[u8]) {
        // Epoch change → host resets cache.
        if epoch != self.epoch {
            self.epoch = epoch;
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

        // Assert the invariant: reconstructed == full_payload.
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

// ── Group A: every visible-output mutation MUST emit ─────────────────────────

/// A.1 — First emission after construction is always a full emit.
#[test]
fn a1_first_emission_is_always_full_emit() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0xAA, 58_768);
    let result = state.should_emit(p.clone(), 0);
    assert!(result.is_some(), "first emission must never be omitted");
    let (emitted_payload, rev) = result.unwrap();
    assert_eq!(emitted_payload, p, "first emission must carry the full payload");
    assert_eq!(rev, 1, "first emission rev must be 1");
    assert_eq!(state.current_rev(), 1);
}

/// A.2 — Changed payload (new in-window root / card content edit) → emit,
/// rev bumped.
#[test]
fn a2_changed_payload_emits_and_bumps_rev() {
    let mut state = FeedEmissionState::new(capability_on());
    let p1 = payload(0x01, 58_768);
    let p2 = payload(0x02, 58_768); // different content = new root or edit

    state.should_emit(p1, 0);
    let result = state.should_emit(p2.clone(), 0);
    assert!(result.is_some(), "changed payload must emit");
    let (_, rev) = result.unwrap();
    assert_eq!(rev, 2, "rev must have advanced from 1 -> 2");
}

/// A.3 — Card removal (payload shrinks / content changes) → emit, rev bumped.
#[test]
fn a3_card_removal_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    // Simulate 80-card payload, then one card removed (different bytes).
    state.should_emit(payload(0xFF, 58_872), 0);
    let removed = payload(0xFE, 57_500); // shorter -- one card removed
    let result = state.should_emit(removed, 0);
    assert!(result.is_some(), "card removal must emit");
    let (_, rev) = result.unwrap();
    assert_eq!(rev, 2);
}

/// A.4 — Card reorder (bytes change even if same cards) → emit.
#[test]
fn a4_card_reorder_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    let ordered = {
        let mut v = vec![0u8; 200];
        v[0] = 0xAA;
        v[100] = 0xBB;
        v
    };
    let reordered = {
        let mut v = vec![0u8; 200];
        v[0] = 0xBB; // swapped
        v[100] = 0xAA;
        v
    };
    state.should_emit(ordered, 0);
    let result = state.should_emit(reordered, 0);
    assert!(result.is_some(), "reorder must emit (bytes differ)");
}

/// A.5 — Attribution added to a visible root → payload changes → emit.
#[test]
fn a5_attribution_added_to_visible_root_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    let before = payload(0x10, 58_000); // no attribution
    let after = payload(0x10, 58_736); // with attribution (longer)
    state.should_emit(before, 0);
    let result = state.should_emit(after, 0);
    assert!(result.is_some(), "attribution add must emit");
}

/// A.6 — Attribution removed from a visible root → payload changes → emit.
#[test]
fn a6_attribution_removed_from_visible_root_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    let before = payload(0x10, 58_736); // with attribution
    let after = payload(0x10, 58_000); // without
    state.should_emit(before, 0);
    let result = state.should_emit(after, 0);
    assert!(result.is_some(), "attribution remove must emit");
}

/// A.7 — Profile refresh changing an author display name inside a visible
/// card → bytes change → emit. This is the subtlest input — the one M2
/// would most likely miss because it requires hooking the profile fan-in path.
/// M1's structural trap-proof guarantee means this is covered automatically.
#[test]
fn a7_profile_refresh_in_visible_card_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    // "Alice" in the encoded card bytes changes to "Alice Renamed".
    let before = b"...author:Alice...".to_vec();
    let after = b"...author:Alice Renamed...".to_vec();
    state.should_emit(before, 0);
    let result = state.should_emit(after, 0);
    assert!(result.is_some(), "profile refresh in visible card must emit");
    let (_, rev) = result.unwrap();
    assert_eq!(rev, 2);
}

/// A.8 — Window growth (`load_older`) revealing new cards → bytes change → emit.
#[test]
fn a8_load_older_window_growth_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    let window_80 = payload(0x55, 58_872);
    let window_160 = payload(0x55, 117_744); // double -- more cards
    state.should_emit(window_80, 0);
    let result = state.should_emit(window_160, 0);
    assert!(result.is_some(), "window growth must emit");
    let (_, rev) = result.unwrap();
    assert_eq!(rev, 2);
}

/// A.9 — Identity reset → epoch changes → forced full baseline re-emit.
/// In production the account-switch callback bumps the `emission_epoch` Arc.
#[test]
fn a9_identity_reset_epoch_change_forces_baseline() {
    let mut state = FeedEmissionState::new(capability_on());
    let p1 = payload(0xAA, 58_768);
    state.should_emit(p1.clone(), 0); // epoch 0, rev 1

    // Identity reset bumps epoch from 0 -> 1.
    let p2 = payload(0xBB, 58_768); // new account's feed
    let result = state.should_emit(p2.clone(), 1);
    assert!(result.is_some(), "epoch change must force a baseline re-emit");
    let (emitted, rev) = result.unwrap();
    assert_eq!(emitted, p2);
    // Rev resets to 1 for the new epoch.
    assert_eq!(rev, 1, "rev must reset to 1 after epoch change");
    assert_eq!(state.current_epoch(), 1);
}

/// A.10 — New in-window root: bytes differ because a new root card was
/// appended. The engine snapshot includes it; bytes differ → emit.
#[test]
fn a10_new_in_window_root_emits() {
    let mut state = FeedEmissionState::new(capability_on());
    let no_roots = payload(0x00, 200); // empty feed
    let one_root = payload(0x01, 58_736); // one card
    state.should_emit(no_roots, 0);
    let result = state.should_emit(one_root, 0);
    assert!(result.is_some(), "new in-window root must emit");
    let (_, rev) = result.unwrap();
    assert_eq!(rev, 2);
}

// ── Group B: non-output mutations MUST omit ───────────────────────────────────

/// B.1 — Idle tick (no ingest, payload byte-identical) → omit.
#[test]
fn b1_idle_tick_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0xAA, 58_768);
    state.should_emit(p.clone(), 0); // first emit
    let result = state.should_emit(p, 0); // identical next tick
    assert!(result.is_none(), "idle tick must be omitted");
    assert_eq!(state.current_rev(), 1, "rev must not advance on omit");
}

/// B.2 — Multiple consecutive idle ticks → all omitted, rev stable.
#[test]
fn b2_multiple_idle_ticks_all_omit() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0xCC, 58_768);
    state.should_emit(p.clone(), 0);
    for _ in 0..39 {
        let result = state.should_emit(p.clone(), 0);
        assert!(result.is_none(), "each idle tick must omit");
    }
    assert_eq!(state.current_rev(), 1, "rev stable across 39 idle ticks");
}

/// B.3 — Out-of-window event (ingested beyond the visible 80): because
/// the engine's `snapshot()` only materializes the visible window, encoded
/// bytes are unchanged → omit. Simulated by passing the same payload.
#[test]
fn b3_out_of_window_event_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let window_bytes = payload(0x77, 58_872);
    state.should_emit(window_bytes.clone(), 0);
    // Out-of-window event: visible window unchanged → same bytes.
    let result = state.should_emit(window_bytes, 0);
    assert!(result.is_none(), "out-of-window event must omit");
}

/// B.4 — Duplicate event (already held in engine): engine state unchanged →
/// bytes identical → omit.
#[test]
fn b4_duplicate_event_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0x44, 58_768);
    state.should_emit(p.clone(), 0);
    let result = state.should_emit(p, 0);
    assert!(result.is_none(), "duplicate event must omit");
}

/// B.5 — Attribution added to a non-visible root (beyond window 80): visible
/// window bytes unchanged → omit.
#[test]
fn b5_attribution_on_non_visible_root_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0x33, 58_872); // full window unchanged
    state.should_emit(p.clone(), 0);
    let result = state.should_emit(p, 0);
    assert!(result.is_none(), "non-visible attribution must omit");
}

/// B.6 — Profile refresh for an author NOT rendered in any visible card:
/// visible bytes unchanged → omit.
#[test]
fn b6_profile_refresh_for_non_visible_author_omits() {
    let mut state = FeedEmissionState::new(capability_on());
    let p = payload(0x22, 58_000);
    state.should_emit(p.clone(), 0);
    let result = state.should_emit(p, 0);
    assert!(result.is_none(), "non-visible profile refresh must omit");
}

// ── Group C: host-coherence simulation ───────────────────────────────────────

/// C.1 — Omit frame: host retains prior value (omit==retain invariant).
#[test]
fn c1_omit_retains_prior_value() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p = payload(0xDE, 58_768);

    // Frame 1: emit (first tick).
    let r1 = state.should_emit(p.clone(), 0);
    host.apply(r1, 0, &p);

    // Frame 2: omit (idle tick). Host must retain p.
    let r2 = state.should_emit(p.clone(), 0);
    assert!(r2.is_none());
    host.apply(r2, 0, &p); // asserts invariant internally
}

/// C.2 — Changed frame after omit: host overwrites with new value.
#[test]
fn c2_changed_after_omit_overwrites_host_cache() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p1 = payload(0x11, 58_768);
    let p2 = payload(0x22, 58_768);

    let r1 = state.should_emit(p1.clone(), 0);
    host.apply(r1, 0, &p1);

    let idle = state.should_emit(p1.clone(), 0);
    host.apply(idle, 0, &p1);

    let r3 = state.should_emit(p2.clone(), 0);
    assert!(r3.is_some(), "changed payload must emit");
    host.apply(r3, 0, &p2);
}

/// C.3 — Epoch change → host resets cache, then receives baseline.
#[test]
fn c3_epoch_change_resets_host_cache_then_baseline() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p_account_a = payload(0xAA, 58_768);
    let p_account_b = payload(0xBB, 58_768);

    // Account A session (epoch 0).
    let r1 = state.should_emit(p_account_a.clone(), 0);
    host.apply(r1, 0, &p_account_a);

    // Idle tick: account A feed unchanged.
    let idle = state.should_emit(p_account_a.clone(), 0);
    assert!(idle.is_none());
    host.apply(idle, 0, &p_account_a);

    // Account switch → epoch 1. State resets, host resets cache.
    // First post-epoch tick is always a full emit.
    let r3 = state.should_emit(p_account_b.clone(), 1);
    assert!(r3.is_some(), "first tick after epoch change must be a baseline");
    let (_, rev_after_reset) = r3.as_ref().unwrap();
    assert_eq!(*rev_after_reset, 1, "rev resets to 1 after epoch change");
    host.apply(r3, 1, &p_account_b);
}

/// C.4 — Capability OFF: every tick emits (byte-identical to today).
/// This verifies the capability-OFF path produces no omission at all.
#[test]
fn c4_capability_off_always_emits() {
    let mut state = FeedEmissionState::new(capability_off());
    let p = payload(0xAA, 58_768);

    // 40 idle ticks -- all must emit (capability OFF = no omission).
    for i in 1..=40 {
        let result = state.should_emit(p.clone(), 0);
        assert!(result.is_some(), "capability OFF must always emit (tick {i})");
    }
}

/// C.5 — Reorder guard: a stale-rev frame (rev <= cached_rev) is rejected
/// by the host. Tests that monotonic rev keeps the reorder guard correct.
#[test]
fn c5_monotonic_rev_keeps_reorder_guard_correct() {
    let mut state = FeedEmissionState::new(capability_on());
    let p1 = payload(0x10, 58_768);
    let p2 = payload(0x20, 58_768);

    let (_, rev1) = state.should_emit(p1, 0).unwrap();
    let (_, rev2) = state.should_emit(p2, 0).unwrap();
    assert!(rev2 > rev1, "rev must be strictly monotonically increasing");
}

/// C.6 — Full host-coherence sequence: interleave emits and omits across
/// 5 unique payloads, each repeated 3x. Host reconstruction must always
/// match the true full payload.
#[test]
fn c6_host_coherence_full_sequence() {
    let cap = capability_on();
    let mut state = FeedEmissionState::new(Arc::clone(&cap));
    let mut host = HostCacheSim::new();

    let payloads: Vec<Vec<u8>> = (0u8..5)
        .flat_map(|tag| {
            // Each "tag" produces a unique payload; repeat it 3x to create omit runs.
            std::iter::repeat(payload(tag, 58_768 + tag as usize)).take(3)
        })
        .collect();

    for p in payloads.iter() {
        let r = state.should_emit(p.clone(), 0);
        // The true full payload is always `p` (what the engine produced this tick).
        // HostCacheSim::apply asserts host == p internally.
        host.apply(r, 0, p);
    }
}

/// C.7 — Capability OFF is byte-identical to today: all ticks including idle
/// ones emit, with monotonically increasing revs.
#[test]
fn c7_capability_off_is_byte_identical_to_today() {
    let mut state = FeedEmissionState::new(capability_off());
    let p = payload(0xFF, 58_768);

    let r1 = state.should_emit(p.clone(), 0).expect("must emit tick 1");
    let r2 = state
        .should_emit(p.clone(), 0)
        .expect("must emit tick 2 (idle, cap off)");
    let r3 = state
        .should_emit(p.clone(), 0)
        .expect("must emit tick 3 (idle, cap off)");

    // All payloads identical (capability OFF emits everything).
    assert_eq!(r1.0, p);
    assert_eq!(r2.0, p);
    assert_eq!(r3.0, p);

    // Revs monotonically increasing (state advances even on cap-off idle ticks).
    assert!(r2.1 > r1.1);
    assert!(r3.1 > r2.1);
}

/// C.8 — 100-tick sequence with interleaved changes every 10 ticks (capability ON).
/// Host reconstruction must match the full payload on every tick.
#[test]
fn c8_long_sequence_host_coherence() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let mut current_payload = payload(0x00, 58_768);

    for tick in 0u64..100 {
        // Every 10 ticks, "change" the feed (new event ingested in window).
        if tick % 10 == 0 {
            current_payload = payload((tick % 256) as u8, 58_768 + (tick as usize % 100));
        }
        let r = state.should_emit(current_payload.clone(), 0);
        host.apply(r, 0, &current_payload);
    }
}

/// C.9 — Capability flag can be read from an AtomicBool shared across threads.
/// Verifies that `Arc<AtomicBool>` (the production wiring pattern where the
/// main thread sets the flag and the actor thread reads it in the closure)
/// correctly propagates the capability declaration.
#[test]
fn c9_capability_flag_propagates_from_shared_atomic() {
    let flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // Closure captures the same Arc the "main thread" holds.
    let mut state = FeedEmissionState::new(Arc::clone(&flag));
    let p = payload(0xAB, 58_768);

    // Before declaration: capability OFF → always emits.
    let r1 = state.should_emit(p.clone(), 0);
    let r2 = state.should_emit(p.clone(), 0);
    assert!(r1.is_some(), "cap OFF → emit tick 1");
    assert!(r2.is_some(), "cap OFF → emit tick 2 (idle)");

    // Simulate `declare_incremental_apply` setting the flag.
    flag.store(true, Ordering::Release);

    // After declaration: capability ON → idle tick omits.
    let r3 = state.should_emit(p.clone(), 0);
    assert!(
        r3.is_none(),
        "cap ON after flag store → idle tick omits (bytes same as last emission)"
    );
}
