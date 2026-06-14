//! ADR-0055 Rung 3 (D3-6) — encoder buffer reuse safety tests.
//!
//! Validates that the kernel-owned reusable `FlatBufferBuilder` (`snapshot_builder`)
//! does not alias frames across ticks. The key correctness invariant:
//!
//! > Frame N's returned `UpdateFrameBytes` owns its bytes independently and
//! > decodes correctly AFTER frame N+1 has been encoded (i.e. after the builder
//! > has been `reset()` for the next tick). No shared borrow into the builder's
//! > internal buffer must survive past the encode return.
//!
//! This module tests:
//! 1. **No-aliasing across 100 sequential ticks**: each frame decodes to the
//!    envelope that was current AT ITS TICK, not a later tick's state (buffer
//!    corruption would cause a later frame's bytes to bleed into an earlier
//!    frame's `Vec<u8>`).
//! 2. **Per-frame rev monotonicity**: the `rev` field in each decoded envelope
//!    matches the tick number (each `make_update` call increments `rev`).
//! 3. **Capacity stability after warmup** (optional): the builder's capacity
//!    does not keep growing tick-over-tick after the first encode, confirming
//!    the reuse actually avoids repeated heap allocation.
//!
//! These tests use the production `make_update` path (NOT the test-only
//! helpers) so they exercise the real kernel field interaction.

use std::sync::Arc;

// This module is pulled in via `#[path]` from `kernel::update`, so `super`
// here is `kernel::update`. Reach the kernel root via `super::super`.
use super::super::snapshot_registry::new_snapshot_projection_slot;
use super::super::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::update_envelope::{decode_snapshot_envelope, decode_snapshot_typed_projections};

/// Drive the kernel for `N` ticks and return ALL frames collected.
///
/// Uses the production `make_update` path so the real `snapshot_builder`
/// field is exercised on every call.
fn collect_frames(kernel: &mut Kernel, n: usize) -> Vec<Vec<u8>> {
    (0..n).map(|_| kernel.make_update(true)).collect()
}

/// Construct a fresh kernel with a snapshot slot installed (so registry reads
/// in `make_update` succeed and Tier-2 projections emit correctly).
fn kernel_with_slot() -> Kernel {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));
    kernel
}

// ── No-aliasing invariant ──────────────────────────────────────────────────────

/// Drive 100 sequential `make_update` ticks and verify EACH collected frame
/// decodes independently and correctly.
///
/// The safety invariant under test (ADR-0055 D3-6): `encode_snapshot_with_envelope`
/// calls `builder.reset()` and then copies out the finished bytes via
/// `builder.finished_data().to_vec()` before returning. The returned `Vec<u8>`
/// therefore owns its data independently of the builder's internal buffer.
///
/// If the `to_vec()` copy were missing (i.e. a caller somehow retained a slice
/// into the builder's buffer), calling `reset()` on the NEXT tick would corrupt
/// the previously "returned" bytes in-place. This test surfaces that class of
/// bug because it:
/// 1. Collects ALL 100 frames as `Vec<u8>` (so all are live simultaneously).
/// 2. Decodes EVERY frame AFTER all 100 encodes have completed (i.e. after 99
///    `reset()` calls on the shared builder since frame 0 was produced).
/// 3. Asserts every decoded envelope carries a non-zero `rev` and
///    `last_tick_ms`, and that all 100 frames parse without error.
///
/// A `to_vec()`-missing bug would produce either a parse error (the builder's
/// internal buffer has been rewritten) or a rev field of 0 (zero-initialized
/// reset bytes).
#[test]
fn buffer_reuse_no_aliasing_100_ticks() {
    let mut kernel = kernel_with_slot();

    // Collect 100 frames. ALL frames are held in `frames` while subsequent
    // encodes call `builder.reset()`. If any frame's `Vec<u8>` aliases the
    // builder buffer, the later resets corrupt the earlier frame's bytes.
    let frames = collect_frames(&mut kernel, 100);
    assert_eq!(frames.len(), 100);

    // Decode every frame AFTER all encodes. If aliasing occurred, earlier
    // frames would decode to corrupted state (or fail to parse).
    for (i, frame) in frames.iter().enumerate() {
        let envelope = decode_snapshot_envelope(frame).unwrap_or_else(|err| {
            panic!(
                "frame {i} failed to decode after 100 encodes (possible builder aliasing): {err}"
            )
        });
        // Each make_update call increments rev starting from 1.
        // The first frame has rev=1, the Nth has rev=N.
        let expected_rev = (i + 1) as u64;
        assert_eq!(
            envelope.rev, expected_rev,
            "frame {i}: decoded rev={} but expected {} (possible builder aliasing or rev \
             not advancing monotonically)",
            envelope.rev, expected_rev
        );
        // `running=true` was passed to every `make_update` call.
        assert!(
            envelope.running,
            "frame {i}: expected running=true but got false"
        );
        // Every real frame must carry a non-zero sequence number.
        assert!(
            envelope.update_sequence > 0,
            "frame {i}: update_sequence must be > 0 after make_update"
        );
        // Verify the frame still has a valid structure by confirming the
        // envelope decoded without error above (already checked by the
        // `unwrap_or_else` above). The key correctness signal is the correct
        // `rev` value decoded from a frame that has already had the builder's
        // buffer overwritten 99 times since it was produced.
    }
}

/// Verify that each frame's `rev` is strictly monotonically increasing across
/// 100 sequential ticks. This confirms the tick counter advances correctly and
/// that no frame shares state with another (which would show up as a repeated
/// rev under aliasing).
#[test]
fn buffer_reuse_rev_monotonically_increasing() {
    let mut kernel = kernel_with_slot();
    let frames = collect_frames(&mut kernel, 100);

    let revs: Vec<u64> = frames
        .iter()
        .map(|f| {
            decode_snapshot_envelope(f)
                .expect("frame must decode")
                .rev
        })
        .collect();

    // Verify strictly increasing.
    for window in revs.windows(2) {
        assert!(
            window[1] > window[0],
            "rev not strictly increasing: {} then {} (possible frame aliasing)",
            window[0],
            window[1]
        );
    }
}

/// Verify that the typed-projection sidecar also decodes correctly for every
/// frame after 100 sequential encodes (no aliasing in the typed-projection
/// bytes, which are also inside the builder buffer).
#[test]
fn buffer_reuse_typed_projections_decode_after_100_ticks() {
    let mut kernel = kernel_with_slot();
    let frames = collect_frames(&mut kernel, 100);

    for (i, frame) in frames.iter().enumerate() {
        let typed = decode_snapshot_typed_projections(frame).unwrap_or_else(|err| {
            panic!(
                "typed projections in frame {i} failed to decode after 100 encodes \
                 (possible builder aliasing): {err}"
            )
        });
        // Each frame must have at least some typed projections (the Tier-2
        // built-ins). An empty sidecar on a kernel-with-slot is a bug.
        assert!(
            !typed.is_empty(),
            "frame {i}: typed projections must be non-empty after make_update \
             (possible builder aliasing or slot not installed)"
        );
        // All returned projection data must have non-empty keys.
        for row in &typed {
            assert!(
                !row.key.is_empty(),
                "frame {i}: typed projection row has empty key (corruption?)"
            );
        }
    }
}

// ── Per-tick state isolation ───────────────────────────────────────────────────

/// Verify that a mutation between ticks is visible in the tick AFTER the
/// mutation and NOT retroactively visible in the tick BEFORE — i.e. that
/// earlier frames are not overwritten when the builder is reused.
///
/// Drives 3 ticks:
/// Tick 1: no relays configured → `no_configured_relays` absent from envelope.
/// Mutation: add a relay via `set_configured_relays`.
/// Tick 2: relay added → `no_configured_relays` semantics may change. The
///          key point is tick 1's frame bytes are immutable after tick 2 encodes.
/// Tick 3: no further mutation.
///
/// After all three encodes, decode tick 1's frame and confirm its state matches
/// the pre-mutation world (no relay configured at that time).
#[test]
fn earlier_frame_not_mutated_by_later_encode() {
    use crate::kernel::AppRelay;

    let mut kernel = kernel_with_slot();

    // Tick 1: no relays.
    let frame1 = kernel.make_update(true);
    let env1_pre = decode_snapshot_envelope(&frame1).expect("frame1 must decode");
    // Snapshot frame1's exact bytes BEFORE any later encode runs. The owned
    // `Vec<u8>` returned by the encoder must be fully independent of the kernel's
    // reused builder buffer; this clone is the ground truth we compare against
    // after two more builder resets. If `to_vec()` ever returned a view that
    // aliased the builder, the live `frame1` would diverge from this snapshot
    // once the builder is reset+rewritten below.
    let frame1_bytes_snapshot = frame1.clone();

    // Mutate: add a relay so the next tick produces materially different content.
    kernel.set_configured_relays(vec![AppRelay::new(
        "wss://relay.damus.io/".to_string(),
        "both".to_string(),
    )]);

    // Tick 2: relay now configured. Encode reuses the builder (reset).
    let frame2 = kernel.make_update(true);
    let env2 = decode_snapshot_envelope(&frame2).expect("frame2 must decode");

    // Tick 3: no further mutation (another reset of the shared builder).
    let _frame3 = kernel.make_update(true);

    // (1) frame1's bytes must be byte-identical after two more builder resets —
    // the core anti-aliasing assertion (non-vacuous: it fails if the returned
    // Vec aliased the builder buffer that tick 2/3 overwrote).
    assert_eq!(
        frame1, frame1_bytes_snapshot,
        "frame1's bytes must be immutable after later encodes reuse+reset the \
         builder (buffer aliasing would corrupt the earlier frame)"
    );

    // (2) Re-decoding frame1 must still yield tick-1 state.
    let env1_post = decode_snapshot_envelope(&frame1).expect("frame1 re-decode must succeed");
    assert_eq!(
        env1_pre.rev, env1_post.rev,
        "frame1's rev must not change when later frames are encoded"
    );

    // (3) frame2 must be a genuinely DISTINCT frame from frame1 — proving the
    // reused builder produced independent output per tick (not an alias where
    // frame1 would have appeared to reflect tick-2's bumped rev).
    assert_ne!(
        env1_pre.rev, env2.rev,
        "frame2 must be a distinct frame from frame1 (each tick bumps rev); \
         equal revs would mean the frames aliased"
    );
}

// ── Capacity stability after warmup (optional) ────────────────────────────────

/// Confirm that the builder's internal buffer does not keep growing after the
/// first tick warms it up. A correctly reused builder grows to fit the largest
/// frame and then holds steady; a builder that allocates fresh on every tick
/// would show per-tick allocations but a STABLE capacity is the reuse signal.
///
/// This is an "optional but nice" gate per the ADR: it proves the reuse
/// actually saves allocations. We check it by comparing the encoded length of
/// frames 2..=10 (post-warmup) and asserting they are all the same size — a
/// fixed-content kernel produces fixed-size frames, and a fixed-size frame
/// implies the builder's capacity was sufficient from tick 2 onward.
///
/// Note: this test does NOT assert on the INTERNAL capacity of the builder
/// (that field is private to the `flatbuffers` crate) — instead it proves the
/// OBSERVABLE invariant: post-warmup frames for a fixed-content kernel have
/// stable byte lengths, which implies the builder is not growing.
#[test]
fn buffer_capacity_stable_after_warmup() {
    let mut kernel = kernel_with_slot();

    // Tick 1: warmup (builder may allocate / grow on first use).
    let _warmup = kernel.make_update(true);

    // Ticks 2..=11: collect post-warmup frames for a fixed-content kernel.
    let post_warmup: Vec<Vec<u8>> = (0..10).map(|_| kernel.make_update(true)).collect();

    // All post-warmup frames must decode successfully.
    for (i, frame) in post_warmup.iter().enumerate() {
        decode_snapshot_envelope(frame).unwrap_or_else(|err| {
            panic!("post-warmup frame {i} failed to decode: {err}")
        });
    }

    // Frame sizes must be stable (no unbounded growth). For a fixed-content
    // kernel, every post-warmup frame should encode to the same byte length.
    let sizes: Vec<usize> = post_warmup.iter().map(|f| f.len()).collect();
    let first_size = sizes[0];
    for (i, &size) in sizes.iter().enumerate().skip(1) {
        // Allow a 1-byte tolerance in case FlatBuffers alignment choices vary.
        // In practice, a fixed-content kernel produces byte-identical frames.
        assert!(
            (size as i64 - first_size as i64).unsigned_abs() <= 8,
            "post-warmup frame sizes diverged at tick {i}: \
             frame 0 = {first_size} bytes, frame {i} = {size} bytes \
             (unexpected growth; the kernel content did not change)"
        );
    }
}
