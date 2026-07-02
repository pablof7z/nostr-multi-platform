//! Cardinal-trap tests for [`TypedProjectionEmissionState`] — ADR-0070 Rung 6 S2.
//!
//! These tests validate the generic whole-value omit mechanism that is SHARED
//! by an app-owned feed session (R6-S1), `refs.event.envelopes` (R6-S2), and
//! `nip46_onboarding` (R6-S2). One implementation, one test suite.
//!
//! Group A: value changes → emit + rev bump (trap-proof: any byte change emits).
//! Group B: value unchanged → omit, rev stable.
//! Group C: host-coherence simulation + freeze guard (the cardinal R6-S1 fix).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{FrameIdentity, TypedProjectionEmissionState};

// ── Host-coherence simulation helper ─────────────────────────────────────────

/// Simulates the host `ProjectionCache` omit==retain + reset semantics.
///
/// Mirrors `ProjectionCache.generated.swift`: a `Changed` frame overwrites; an
/// omit (absent key) retains the prior value; a frame whose `(session_id,
/// snapshot_epoch)` differs from the cached identity triggers `removeAll()`
/// BEFORE the frame is applied.
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
    /// * `result` — the `should_emit` return: `Some((payload, rev))` or `None`.
    /// * `identity` — this frame's `(session_id, snapshot_epoch)`.
    /// * `full_payload` — the true full payload this tick (what the host SHOULD
    ///   hold after this frame, regardless of omit/emit).
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
            "host-coherence invariant violated: reconstructed projection \
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
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let p = payload(0xAA, 1_024);
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

/// A.2 — Changed payload (any byte differs) → emit.
#[test]
fn a2_changed_payload_emits_and_bumps_rev() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    state.should_emit(payload(0x01, 512), id0());
    let result = state.should_emit(payload(0x02, 512), id0());
    assert!(result.is_some(), "changed payload must emit");
    assert_eq!(result.unwrap().1, 2, "rev must advance 1 -> 2");
}

/// A.3 — Payload shrinks (value became smaller) → emit.
#[test]
fn a3_payload_shrink_emits() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    state.should_emit(payload(0xFF, 1_024), id0());
    let result = state.should_emit(payload(0xFF, 512), id0());
    assert!(result.is_some(), "size change must emit");
    assert_eq!(result.unwrap().1, 2);
}

/// A.4 — Account switch → snapshot_epoch bumps → forced full baseline re-emit
/// with rev reset to 1.
#[test]
fn a4_account_switch_epoch_change_forces_baseline() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    state.should_emit(payload(0xAA, 512), id0());

    let switched = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 1,
    };
    let p2 = payload(0xBB, 512);
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

/// A.5 — Payload content changes (not just size) → emit.
#[test]
fn a5_content_change_emits() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    state.should_emit(b"...stage:idle...".to_vec(), id0());
    let result = state.should_emit(b"...stage:in_flight...".to_vec(), id0());
    assert!(result.is_some(), "content change must emit");
    assert_eq!(result.unwrap().1, 2);
}

// ── Group B: unchanged value MUST omit ───────────────────────────────────────

/// B.1 — Idle tick (no mutation, payload byte-identical) → omit.
#[test]
fn b1_idle_tick_omits() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let p = payload(0xAA, 512);
    state.should_emit(p.clone(), id0());
    assert!(state.should_emit(p, id0()).is_none(), "idle tick must omit");
    assert_eq!(state.current_rev(), 1, "rev must not advance on omit");
}

/// B.2 — Multiple consecutive idle ticks → all omitted, rev stable.
#[test]
fn b2_multiple_idle_ticks_all_omit() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let p = payload(0xCC, 512);
    state.should_emit(p.clone(), id0());
    for tick in 0..39 {
        assert!(
            state.should_emit(p.clone(), id0()).is_none(),
            "idle tick {tick} must omit"
        );
    }
    assert_eq!(state.current_rev(), 1, "rev stable across 39 idle ticks");
}

/// B.3 — Same value after a change → omit again.
#[test]
fn b3_back_to_same_value_omits() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let p1 = payload(0x11, 512);
    let p2 = payload(0x22, 512);
    state.should_emit(p1.clone(), id0());
    state.should_emit(p2.clone(), id0());
    // Now back to p2 (same as last emitted)
    assert!(
        state.should_emit(p2, id0()).is_none(),
        "same-as-last must omit"
    );
}

// ── Group C: host-coherence simulation + freeze guard ───────────────────────

/// C.1 — Omit frame: host retains prior value (omit==retain invariant).
#[test]
fn c1_omit_retains_prior_value() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p = payload(0xDE, 512);
    host.apply(state.should_emit(p.clone(), id0()), id0(), &p);
    let r2 = state.should_emit(p.clone(), id0());
    assert!(r2.is_none());
    host.apply(r2, id0(), &p);
}

/// C.2 — Changed frame after omit: host overwrites with new value.
#[test]
fn c2_changed_after_omit_overwrites_host_cache() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p1 = payload(0x11, 512);
    let p2 = payload(0x22, 512);
    host.apply(state.should_emit(p1.clone(), id0()), id0(), &p1);
    host.apply(state.should_emit(p1.clone(), id0()), id0(), &p1); // omit
    let r3 = state.should_emit(p2.clone(), id0());
    assert!(r3.is_some(), "changed payload must emit");
    host.apply(r3, id0(), &p2);
}

/// C.3 — Account-switch epoch change → host resets cache, then baseline.
#[test]
fn c3_epoch_change_resets_host_cache_then_baseline() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let pa = payload(0xAA, 512);
    let pb = payload(0xBB, 512);
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
    let mut state = TypedProjectionEmissionState::new(capability_off());
    let p = payload(0xAA, 512);
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
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let rev1 = state.should_emit(payload(0x10, 512), id0()).unwrap().1;
    let rev2 = state.should_emit(payload(0x20, 512), id0()).unwrap().1;
    assert!(rev2 > rev1, "rev must be strictly monotonically increasing");
}

/// C.6 — Freeze guard, session_id axis (THE FREEZE TEST).
/// `ActorCommand::Lifecycle(LifecycleCommand::Reset)` rebuilds the kernel → new `session_id`, but the
/// producer's `TypedProjectionEmissionState` SURVIVES, so the next tick may
/// encode BYTE-IDENTICAL bytes. The host cache reset (new session_id →
/// removeAll) means an omit here would leave the host with NO projection entry.
///
/// Against a naive impl that only checks byte equality (no identity), the
/// producer would OMIT and this test would FAIL. With the freeze fix
/// (identity = `(session_id, snapshot_epoch)`), the changed `session_id` forces
/// a baseline.
#[test]
fn c6_freeze_guard_session_id_change_forces_baseline() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p = payload(0xC0, 512);

    let pre = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    };
    host.apply(state.should_emit(p.clone(), pre), pre, &p);
    let idle = state.should_emit(p.clone(), pre);
    assert!(idle.is_none(), "pre-Reset idle tick omits");
    host.apply(idle, pre, &p);

    // Reset: kernel rebuild → new session_id. Producer state + bytes survive.
    let post = FrameIdentity {
        session_id: 2_000, // new kernel run
        snapshot_epoch: 0,
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
    host.apply(post_reset, post, &p);
}

/// C.7 — Freeze guard, epoch axis: a `snapshot_epoch` change with byte-identical
/// payload (account switch where the new account's value encodes identically —
/// e.g. both empty) MUST emit a baseline, not omit.
#[test]
fn c7_freeze_guard_epoch_change_identical_bytes_forces_baseline() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let empty = payload(0x00, 64); // both accounts' empty value encodes the same

    let a = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    };
    host.apply(state.should_emit(empty.clone(), a), a, &empty);

    // Account switch: epoch bumps, but value encodes to IDENTICAL bytes.
    let b = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 1,
    };
    let switched = state.should_emit(empty.clone(), b);
    assert!(
        switched.is_some(),
        "FREEZE GUARD: identical bytes after an epoch change MUST emit a baseline"
    );
    host.apply(switched, b, &empty);
}

/// C.8 — Capability flag propagates from a shared AtomicBool.
#[test]
fn c8_capability_flag_propagates_from_shared_atomic() {
    let flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let mut state = TypedProjectionEmissionState::new(Arc::clone(&flag));
    let p = payload(0xAB, 512);
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

/// C.9 — Long sequence with periodic changes.
#[test]
fn c9_long_sequence_host_coherence() {
    let mut state = TypedProjectionEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let mut current = payload(0x00, 512);
    for tick in 0u64..100 {
        if tick % 10 == 0 {
            current = payload((tick % 256) as u8, 512 + (tick as usize % 64));
        }
        host.apply(state.should_emit(current.clone(), id0()), id0(), &current);
    }
}
