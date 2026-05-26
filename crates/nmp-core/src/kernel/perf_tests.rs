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
//! ## Threshold rationale
//!
//! Observed baseline on developer hardware (Apple M-series, debug build,
//! 1k-event firehose, `visible_limit = 500`, 5-run average):
//! - `make_update_us` ≈ 25 ms
//! - `serialize_us`   ≈ 15 ms
//! - run-to-run variance < 5 %
//!
//! `cargo test` in `test.yml` runs **debug** mode on `ubuntu-latest` shared
//! runners — typically 2–3 × slower than dev hardware, plus neighbor-noise
//! jitter. Ceilings are set at ≈ 10 × observed so a real 10 × regression
//! (the kind that breaks 4 Hz iOS cadence) fails the build while ordinary
//! CI variance does not flake:
//! - `MAX_MAKE_UPDATE_US = 250_000` (250 ms)
//! - `MAX_SERIALIZE_US   = 150_000` (150 ms)
//!
//! The real monitoring signal is the `NMP_PERF` log line emitted on every
//! tick in production; this gate is the coarse net that catches a snapshot
//! path that has clearly broken. Tighten the ceiling only if a follow-up
//! perf budget is documented in `docs/plan.md` or `BACKLOG.md`.
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
/// 250 ms = ≈ 10 × the observed dev-hardware baseline (~25 ms debug). Picked
/// to catch a 10 × regression on shared CI runners while leaving headroom
/// for the typical 2–3 × debug + ubuntu-latest slowdown plus normal
/// neighbor-noise jitter. See the module-level threshold rationale.
const MAX_MAKE_UPDATE_US: u128 = 250_000;

/// Upper bound for `serialize_us` (the FlatBuffers encode tail alone). 150 ms
/// = ≈ 10 × the observed pre-FlatBuffers dev-hardware JSON baseline (~15 ms
/// debug). Same rationale as `MAX_MAKE_UPDATE_US`: a 150 ms encode on a
/// 1k-event snapshot indicates the projections map or `KernelSnapshot` shape
/// has regressed past acceptable steady-state.
const MAX_SERIALIZE_US: u128 = 150_000;

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
            let nostr_event = ::nostr::EventBuilder::text_note(format!(
                "snapshot perf firehose note {i}"
            ))
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
