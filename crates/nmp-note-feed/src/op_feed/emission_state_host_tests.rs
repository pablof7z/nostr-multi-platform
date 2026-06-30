//! Host-coherence tests for [`FeedEmissionState`] - ADR-0055 Rung 6 S1.
//!
//! Group C simulates host cache reconstruction across emit/omit/identity-reset
//! sequences. The reconstructed host feed must always match the full payload.
//! These tests also cover the R6-S1 freeze fix: a host cache reset
//! (`session_id` OR `snapshot_epoch` change) while the producer state is
//! preserved must force a producer baseline, not an omit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{FeedEmissionState, FrameIdentity};

/// Simulates the host `ProjectionCache` omit==retain + reset semantics.
///
/// Mirrors `ProjectionCache.generated.swift`: a `Changed` frame overwrites; an
/// omit (absent key) retains the prior value; a frame whose `(session_id,
/// snapshot_epoch)` differs from the cached identity triggers `removeAll()`
/// before the frame is applied.
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

    fn apply(
        &mut self,
        result: Option<(Vec<u8>, u64)>,
        identity: FrameIdentity,
        full_payload: &[u8],
    ) {
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
            None => {}
        }

        assert_eq!(
            self.cached.as_deref().unwrap_or(&[]),
            full_payload,
            "host-coherence invariant violated: reconstructed feed \
             does not match the full-emit payload"
        );
    }
}

fn capability_on() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(true))
}

fn capability_off() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn payload(tag: u8, size: usize) -> Vec<u8> {
    vec![tag; size]
}

fn id0() -> FrameIdentity {
    FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    }
}

/// C.1 - Omit frame: host retains prior value (omit==retain invariant).
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

/// C.2 - Changed frame after omit: host overwrites with new value.
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

/// C.3 - Account-switch epoch change -> host resets cache, then baseline.
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

/// C.4 - Capability OFF: every tick emits (byte-identical to today).
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

/// C.5 - Monotonic rev keeps the host reorder guard correct.
#[test]
fn c5_monotonic_rev_keeps_reorder_guard_correct() {
    let mut state = FeedEmissionState::new(capability_on());
    let rev1 = state.should_emit(payload(0x10, 58_768), id0()).unwrap().1;
    let rev2 = state.should_emit(payload(0x20, 58_768), id0()).unwrap().1;
    assert!(rev2 > rev1, "rev must be strictly monotonically increasing");
}

/// C.6 - Full host-coherence sequence: 5 unique payloads, each repeated 3x.
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

/// C.7 - Capability OFF is byte-identical to today: idle ticks still emit with
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

/// C.8 - 100-tick sequence with changes every 10 ticks.
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

/// C.9 - Capability flag propagates from a shared AtomicBool (the production
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
    flag.store(true, Ordering::Release);
    assert!(
        state.should_emit(p.clone(), id0()).is_none(),
        "cap ON after flag store -> idle tick omits"
    );
}

/// C.10 - THE FREEZE TEST (R6-S1 kill criterion). `ActorCommand::Lifecycle(LifecycleCommand::Reset)` rebuilds
/// the kernel -> new `session_id`, but the producer's `FeedEmissionState` and the
/// engine `Arc` survive, so the next tick encodes byte-identical bytes. The host
/// cache reset means an omit here would leave the host with no feed entry.
#[test]
fn c10_reset_new_session_id_forces_baseline_not_omit() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let p = payload(0xC0, 58_768);

    let pre = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    };
    host.apply(state.should_emit(p.clone(), pre), pre, &p);
    let idle = state.should_emit(p.clone(), pre);
    assert!(idle.is_none(), "pre-Reset idle tick omits");
    host.apply(idle, pre, &p);

    let post = FrameIdentity {
        session_id: 2_000,
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

/// C.11 - Freeze guard, epoch axis: a `snapshot_epoch` change with byte-identical
/// payload must emit a baseline, not omit.
#[test]
fn c11_epoch_change_identical_bytes_forces_baseline_not_omit() {
    let mut state = FeedEmissionState::new(capability_on());
    let mut host = HostCacheSim::new();
    let empty_feed = payload(0x00, 200);

    let a = FrameIdentity {
        session_id: 1_000,
        snapshot_epoch: 0,
    };
    host.apply(state.should_emit(empty_feed.clone(), a), a, &empty_feed);

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
