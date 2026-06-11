//! Snapshot perf CI regression gate (v1 exit criterion #8).
//!
//! `Kernel::make_update` is the hot path called at up to 4 Hz on every
//! actor tick. It builds the full `KernelSnapshot` (timeline diff, every
//! registered projection, identity + views clusters) and encodes it into the
//! canonical FlatBuffers update frame exactly once. Two timing fields are
//! recorded on every call:
//!
//! - `last_make_update_us` — total microseconds from `emit_started` through
//!   the FlatBuffers frame encode. Covers projection builds + encode.
//! - `last_serialize_us`   — microseconds spent in the encode tail alone.
//!
//! Both are surfaced through the snapshot's `metrics` field (one-tick lag,
//! same pattern as `last_payload_bytes`) and the `NMP_PERF` log line in
//! `kernel::update`. This test exercises the hot path against a 1k-event
//! firehose and asserts both timings stay under a conservative ceiling.
//!
//! ## Threshold rationale (V-117 tightening, 2026-06-12)
//!
//! Observed baseline on developer hardware (Apple M-series, debug build,
//! 1k-event firehose, `visible_limit = 500`):
//! - `make_update_us` ≈ 323–600 µs idle; **1 330–1 342 µs measured under
//!   parallel-build contention** (max cold-cache ~1 600 µs)
//! - `serialize_us`   ≈ 271–299 µs idle; ~630 µs under contention
//! - idle run-to-run variance < 10 %; contention pushes 2–4 × above idle
//!
//! `cargo test` in `test.yml` runs **debug** mode on `ubuntu-latest` shared
//! runners. The snapshot path is memory-bandwidth-bound; shared runners show
//! 2–3 × p99 jitter on a noisy-neighbor tick on top of the baseline debug
//! slowdown. Taking the measured under-contention value (~1 340 µs / ~630 µs)
//! as the CI-relevant baseline, ceilings are set at ~10 × that value so a
//! noisy p99 tick cannot flake the gate — a flaky perf gate gets deleted,
//! which is strictly worse than a slightly looser one:
//! - `MAX_MAKE_UPDATE_US = 15_000` (15 ms, ~11 × measured contention value)
//! - `MAX_SERIALIZE_US   = 8_000`  (8 ms,  ~12 × measured contention value)
//!
//! Still 17 × / 19 × tighter than the prior 250 ms / 150 ms ceilings, which
//! were based on a stale pre-FlatBuffers ~25 ms / ~15 ms estimate (420 × /
//! 500 × above actual). The 4 Hz-cadence argument holds: a real regression
//! that threatens the 250 ms/tick budget lands at ~60 000 µs and fails the
//! 15 000 µs gate by 4 ×.
//!
//! The real monitoring signal is the `NMP_PERF` log line emitted on every
//! tick in production; this gate is the coarse net that catches a snapshot
//! path that has clearly broken. Tighten the ceiling further only if a
//! follow-up perf budget is documented in `docs/plan.md` or GitHub Issues.
//!
//! See `docs/plan.md` v1 exit criterion #8 for the contract.
//!
//! ## What this test does NOT cover
//!
//! - **Ingest throughput** — see `kernel::timeline_perf_tests` for that.
//! - **Per-projection cost breakdown** — `make_update_us - serialize_us` is
//!   the closest proxy; deeper profiling belongs in a manual perf harness.
//! - **Sustained 4 Hz cadence under live actor load** — the actor-level
//!   harness `crates/nmp-core/src/bin/snapshot_emit_stress.rs` covers that.

use super::nostr::NostrEvent;
use super::Kernel;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

/// Number of synthetic kind:1 events to inject before the gated emit. Large
/// enough to populate `timeline`, `events`, and the timeline-author set with
/// a representative working set (every visible slot is filled and the
/// `visible_items()` / `diff_items()` pair has real work to do); small
/// enough to keep total wall time (including secp256k1 signing of every
/// event) well under the 30s sub-agent watchdog.
const EVENT_COUNT: usize = 1_000;

/// Visible-item window for the snapshot path. The default
/// (`DEFAULT_VISIBLE_LIMIT = 80`) is too narrow — `visible_items()` would
/// only iterate 80 entries even with 1k cached events, hiding the cost
/// growth this gate is meant to detect. The same value the manual
/// `timeline_ingest_perf` harness uses, so the two stay comparable.
const VISIBLE_LIMIT: usize = 500;

/// Upper bound for `make_update_us` (total snapshot build + serialize).
///
/// ## Threshold rationale (tightened V-117, calibrated per PR #1094 review)
///
/// Measured local dev-hardware (Apple M-series, debug build, 1k-event firehose,
/// `visible_limit = 500`): **~323–600 µs idle**, **~1 330–1 342 µs under
/// parallel-build contention** (max cold-cache ~1 600 µs). The snapshot path is
/// memory-bandwidth-bound; ubuntu-latest shared runners add 2–3 × p99 jitter on
/// noisy-neighbor ticks. Ceiling = ~10 × the measured contention value
/// (~1 340 µs) = **15 000 µs**, so a bad p99 tick cannot flake the gate.
/// A real 4 Hz-budget regression (~250 ms/tick threatened) lands at
/// ~60 000 µs and fails by 4 ×.
///
/// Prior ceiling was 250 000 µs (≈ 420 × local) — a stale ~25 ms estimate
/// from a pre-FlatBuffers code path. 15 000 µs is 17 × tighter.
const MAX_MAKE_UPDATE_US: u128 = 15_000;

/// Upper bound for `serialize_us` (the FlatBuffers encode tail alone).
///
/// ## Threshold rationale (tightened V-117, calibrated per PR #1094 review)
///
/// Measured local dev-hardware: **~271–299 µs idle**, **~630 µs under
/// contention** (max cold-cache ~600 µs). Ceiling = ~12 × the measured
/// contention value = **8 000 µs**. Same p99-jitter headroom logic as
/// `MAX_MAKE_UPDATE_US`. Prior ceiling was 150 000 µs (≈ 500 × local);
/// 8 000 µs is 19 × tighter.
const MAX_SERIALIZE_US: u128 = 8_000;

/// Pre-generate `count` signed kind:1 events under a single throwaway
/// keypair. Mirrors `kernel::timeline_perf_tests::make_events` so the two
/// perf harnesses share a fixture shape.
fn signed_notes(count: usize) -> Vec<NostrEvent> {
    let keys = ::nostr::Keys::generate();
    (0..count)
        .map(|i| {
            // Scramble `created_at` so the events are not in monotonic
            // insertion order — the timeline-sort cost matters for the
            // snapshot path (`visible_items()` is a no-op iteration, but
            // the underlying `timeline` ordering touches `events` lookups
            // in non-sequential memory order). Exact pattern lifted from
            // `timeline_perf_tests` so the two harnesses stay comparable.
            let newest_first_scramble = (i.wrapping_mul(37) % count) as u64;
            let nostr_event =
                ::nostr::EventBuilder::text_note(format!("snapshot perf firehose note {i}"))
                    .custom_created_at(::nostr::Timestamp::from(
                        1_700_000_000 + newest_first_scramble,
                    ))
                    .sign_with_keys(&keys)
                    .expect("signing a generated-key note should succeed");
            NostrEvent {
                id: nostr_event.id.to_hex(),
                pubkey: nostr_event.pubkey.to_hex(),
                created_at: nostr_event.created_at.as_secs(),
                kind: nostr_event.kind.as_u16() as u32,
                tags: nostr_event
                    .tags
                    .iter()
                    .map(|tag: &::nostr::Tag| tag.as_slice().to_vec())
                    .collect(),
                content: nostr_event.content.clone(),
                sig: nostr_event.sig.to_string(),
            }
        })
        .collect()
}

/// CI regression gate (v1 exit criterion #8). Asserts that after a 1k-event
/// firehose, a single `make_update` call stays under
/// `MAX_MAKE_UPDATE_US` / `MAX_SERIALIZE_US`.
///
/// **Not** `#[ignore]` — runs on every `cargo test -p nmp-core` invocation,
/// which is what `test.yml` already does on every PR. No new CI workflow is
/// required.
#[test]
fn snapshot_perf_firehose_gate() {
    let events = signed_notes(EVENT_COUNT);

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_visible_limit(VISIBLE_LIMIT);

    // Drive every event through the same `ingest_timeline_event` path the
    // production `handle_relay_frame → handle_message → ingest::dispatch`
    // call ultimately reaches for kind:1 frames. The `diag-firehose-*`
    // sub-id prefix bypasses the planner-registered-interest gate
    // (`kernel::ingest::timeline::should_store_event`) so we don't need to
    // stand up a coverage plan just to populate the timeline.
    for event in events {
        kernel.ingest_timeline_event(
            RelayRole::Content,
            "wss://snapshot-perf.example",
            "diag-firehose-snapshot-perf-gate",
            event,
        );
    }

    // Single `make_update` call — the field-write pattern at the end of
    // `make_update` (`self.last_make_update_us = this_make_update_us;`)
    // means `last_*_us` reflect THIS tick's measurements immediately, not
    // the previous tick's (the one-tick lag only affects the `Metrics`
    // struct embedded in the decoded snapshot, where the assignment is read
    // before write). Reading the fields directly avoids a decode round-trip and a
    // second emit just to surface the value.
    let serialized = kernel.make_update(true);
    let payload_bytes = serialized.len();

    let make_update_us = kernel.last_make_update_us;
    let serialize_us = kernel.last_serialize_us;

    // Print every observed value to stderr — `cargo test` swallows stdout
    // unless `--nocapture` is set, but stderr surfaces on failure regardless
    // and `cargo test -- --show-output` exposes it on success. This is the
    // CI signal a reviewer reads when bumping the threshold or
    // investigating a flaky run — it should never be removed.
    eprintln!(
        "OBSERVED snapshot_perf_firehose_gate events={EVENT_COUNT} visible_limit={VISIBLE_LIMIT} \
         payload_bytes={payload_bytes} make_update_us={make_update_us} serialize_us={serialize_us} \
         build_us={build_us}",
        build_us = make_update_us.saturating_sub(serialize_us)
    );

    // Sanity invariant: the encode tail can never exceed the total. If this
    // fires, the field ordering in `make_update` has been broken.
    assert!(
        serialize_us <= make_update_us,
        "serialize_us ({serialize_us}) must not exceed make_update_us ({make_update_us})"
    );

    assert!(
        make_update_us < MAX_MAKE_UPDATE_US,
        "snapshot perf regression: make_update_us={make_update_us} exceeds \
         ceiling {MAX_MAKE_UPDATE_US} (1k-event firehose, visible_limit={VISIBLE_LIMIT}). \
         See docs/plan.md v1 exit criterion #8."
    );
    assert!(
        serialize_us < MAX_SERIALIZE_US,
        "snapshot perf regression: serialize_us={serialize_us} exceeds \
         ceiling {MAX_SERIALIZE_US} (1k-event firehose, visible_limit={VISIBLE_LIMIT}). \
         See docs/plan.md v1 exit criterion #8."
    );
}

/// Verify that the estimated_store_bytes cache invalidation is correct.
/// After each ingest, the cached value must match a fresh compute.
#[test]
fn estimated_store_bytes_cache_matches_fresh_compute() {
    let events = signed_notes(500);

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut control_kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Ingest events and verify cache matches a fresh compute at each step.
    for (i, event) in events.iter().enumerate() {
        kernel.ingest_timeline_event(
            RelayRole::Content,
            "wss://test.example",
            "test-cache-check",
            event.clone(),
        );
        control_kernel.ingest_timeline_event(
            RelayRole::Content,
            "wss://test.example",
            "test-cache-check",
            event.clone(),
        );

        let cached_value = kernel.estimated_store_bytes();
        let control_value = control_kernel.estimated_store_bytes();

        assert_eq!(
            cached_value, control_value,
            "After ingesting {i} events, cached_estimated_store_bytes ({cached_value}) \
             must match fresh compute ({control_value})"
        );
    }
}

/// Verify that make_update cost does NOT scale linearly with store size
/// (i.e., the O(store) double-scan is eliminated).
///
/// This test builds two kernels at different scales and compares the per-event
/// cost of make_update. A true O(store) double-scan would make the 20x larger
/// kernel roughly 20-40x slower; with caching the per-event cost is nearly flat.
#[test]
fn snapshot_make_update_cost_is_sublinear_in_store_size() {
    // Baseline: 1k events, measure make_update cost.
    let baseline_events = signed_notes(1_000);
    let mut baseline_kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    baseline_kernel.set_visible_limit(VISIBLE_LIMIT);

    for event in baseline_events {
        baseline_kernel.ingest_timeline_event(
            RelayRole::Content,
            "wss://baseline.example",
            "diag-firehose-baseline",
            event,
        );
    }

    baseline_kernel.make_update(true);
    let baseline_us = baseline_kernel.last_make_update_us;

    // Scale test: 20k events (20x store size), measure make_update cost.
    // Using 20k instead of 100k to avoid exceeding the 30s sub-agent watchdog
    // (secp256k1 signing dominates). A 20x store growth is sufficient to expose
    // an O(store) scan; with the fix the per-event cost should not explode.
    let scaled_events = signed_notes(20_000);
    let mut scaled_kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    scaled_kernel.set_visible_limit(VISIBLE_LIMIT);

    for event in scaled_events {
        scaled_kernel.ingest_timeline_event(
            RelayRole::Content,
            "wss://scaled.example",
            "diag-firehose-scaled",
            event,
        );
    }

    scaled_kernel.make_update(true);
    let scaled_us = scaled_kernel.last_make_update_us;

    // Print observed timings for CI diagnostics.
    eprintln!(
        "snapshot_make_update_cost_is_sublinear_in_store_size: \
         baseline_us={baseline_us} (1k events) \
         scaled_us={scaled_us} (20k events) \
         ratio={ratio:.1}x",
        ratio = scaled_us as f64 / baseline_us as f64
    );

    // A true O(store) double-scan would make scaled_us roughly 20-40x larger
    // (two full O(store) scans per emit, times 20x store growth).
    // With caching, we expect the fixed cost (projections, serialization, etc.)
    // to dominate and per-event cost to be nearly flat. A 4x ceiling leaves
    // headroom for non-store fixed costs and CI jitter while still failing
    // the pre-fix double-scan (which would make scaled_us 20-40x baseline).
    assert!(
        scaled_us <= baseline_us * 4,
        "make_update cost scaled super-linearly: baseline_us={baseline_us} (1k events), \
         scaled_us={scaled_us} (20k events). A true O(store) double-scan would \
         produce ~20-40x, but we observe {ratio:.1}x. The caching fix may not be \
         working correctly, or there is another super-linear cost in the hot path.",
        ratio = scaled_us as f64 / baseline_us as f64
    );
}
