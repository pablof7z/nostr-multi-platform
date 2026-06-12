//! T114b — per-dispatch retention audit (PD-021 line-11 M10.5 §G-S2 closer).
//!
//! These tests pin the kernel-side invariant that no actor-routed structure
//! grows once-per-dispatch without bound. The S2 drain analysis
//! (`docs/perf/m10.5/s2-drain-analysis.md`) measured ~38 MiB retained heap
//! after a 30 s flood (0.13 % reclaimed by drain) — genuine unbounded growth.
//!
//! ## Post-fix S2 §G-S2 evidence (`docs/perf/m10.5/s2-retention-audit.md`)
//!
//! | Duration | Dispatches | `retained_heap_after_drain_bytes` | Verdict |
//! |---|---:|---:|---|
//! | 10 s | 100 k  | 519,936 B (~0.5 MiB)  | PASS (≤1 MiB) |
//! | 30 s | 300 k  | 157,956 B (~0.15 MiB) | PASS (≤1 MiB) |
//! | 60 s | 600 k  | 519,748 B (~0.5 MiB)  | PASS (≤1 MiB) |
//!
//! Retention is **flat across 6× the dispatch count** (variance is run-to-run
//! drain-detector noise within the 256 KiB stability band). The kernel is
//! bounded by working set, not dispatch count — the D8 invariant.
//!
//! ## Load-bearing fix (per-dispatch emit gate)
//!
//! The dominant retention source was per-dispatch `emit_now` in `dispatch.rs`:
//! every view command unconditionally serialized a full kernel snapshot and
//! pushed it onto the unbounded kernel→listener `mpsc::channel`. Under flood
//! conditions with no UI consumer (`running=false`, the S2 / S1–S5 harness's
//! Configure-not-Start mode), the channel's internal block free-list retained
//! segments long after the snapshot Strings were processed — measured at
//! ~10 B per dispatch. The fix (`actor/tick.rs::maybe_emit_after_dispatch`)
//! routes view-command emits through a `running=true` gate, mirroring the
//! idle-tick path's existing gating contract. Pinned by the two
//! `view_dispatches_*_when_running` regression tests in `tick.rs`.
//!
//! ## Defense-in-depth (this file)
//!
//! The bounded caps below are **defense-in-depth** — not the load-bearing fix
//! for S2 (the emit gate is). They harden the kernel against adversarial /
//! pathological inputs where a long-lived UI churns unique consumer_ids per
//! pubkey, or an FFI burst saturates the actor channel.
//!
//! | Path                         | Structure                          | Pre-fix    | Bound                                |
//! |------------------------------|------------------------------------|------------|---------------------------------------|
//! | bounded FFI channel          | `actor_tx` mpsc                    | unbounded  | `BOUNDED_ACTOR_CMD_CAPACITY=4096` (T114 part 1, `44cbfd2`) — drop-newest + `dispatch_drops_total` |
//! | view-command emit gate       | per-dispatch `emit_now`            | unconditional | `maybe_emit_after_dispatch` skips when `running=false` (this fix — load-bearing) |
//! | `claim_profile`              | `profile_claims[pk]: BTreeSet`     | unbounded  | `MAX_CLAIMS_PER_PUBKEY=256` — drop-newest + `claim_drops_total` |
//! | latency sketch (harness)     | `Vec<u64>` per-sample              | unbounded  | fixed 32-bucket log2 histogram (`s2_dispatch_flood.rs::LatencyHistogram`) — 256 B per thread |
//! | (was: `open_author`)         | deleted — V-112 (ADR-0042)         | —          | —                                          |
//! | (was: `close_author`)        | deleted — V-112 (ADR-0042)         | —          | —                                          |
//! | `release_profile`            | `profile_claims[pk]` (remove)      | bounded    | shrinking only                              |
//! | `open_firehose_tag`          | `diagnostic_firehose: Option`      | bounded    | O(1) — single-slot refcounted              |
//! | `Kernel::log`                | `logs: VecDeque`                   | bounded    | hard cap 80 (`status.rs:314`)               |
//! | `requested_profiles`         | `HashSet<pubkey>`                  | bounded    | keyed by pubkey (O(working-set))            |
//! | `pending_profiles`           | `BTreeSet<pubkey>`                 | bounded    | keyed by pubkey (O(working-set))            |
//! | profile/author/firehose seq  | `u64` counters                     | bounded    | saturating; 8 B fixed                       |
//!
//! Under the S2 spec mix, `claim_drops_total = 0` and `dispatch_drops_total = 0`
//! at flood end — neither cap is being exercised. That is the correct outcome:
//! the working set (50 pubkeys × ≤256 consumers) fits inside both bounds. The
//! caps surface on `Metrics` (`update.rs`) for diagnostic visibility; their
//! unit tests below pin the drop-newest semantics for the pathological cases.
//!
//! Production paths that DO populate `wire_subs` (post-`Start`) are bounded
//! by the planner CLOSE diff (`drain_lifecycle_tick` behind `close_interest`)
//! and the interest registry refcount. (V-112: the legacy
//! `close_subscriptions_with_prefixes` view-close path was deleted.)
//!
//! ## T133 — `wire_subs` row eviction
//!
//! Pre-T133 the row table only ever marked closed subs with
//! `state="closed"` / `closed_by_relay`; the row stayed for diagnostic
//! surfacing, so the table grew without bound across long sessions (every
//! profile-claim, thread-ids/replies, and discovery oneshot completes via
//! EOSE→CLOSE — the high-volume retention source the advisor flagged on
//! T114b). T133 evicts rows at every terminal point:
//!
//! | Trigger                          | Action                          |
//! |----------------------------------|---------------------------------|
//! | EOSE for non-keep sub (oneshot)  | `HashMap::remove` after CLOSE outbound |
//! | CLOSED (relay-initiated)         | `HashMap::remove` (no outbound)        |
//! | `relay_closed` (per-URL socket teardown) | `wire_subs.retain(relay_url != …)` |
//! | `relay_closed_all` (global pool drain)   | `wire_subs.retain(role != …)`      |
//! | `relay_failed` (transient)       | no eviction — `state="retrying"` may resume |
//!
//! Pinned by `eose_evicts_wire_sub_row` and `closed_frame_evicts_wire_sub_row`
//! below; `view_close_evicts_wire_subs_to_zero` deleted (V-112 — used
//! `open_author`/`close_author` which are deleted). The diagnostic-filter
//! call sites at
//! `status.rs:27` / `requests/mod.rs:25,39,80` remain (defense-in-depth —
//! they cost nothing once the row is gone).

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::atomic::{AtomicU64, Ordering};

/// Construct a 64-char hex pubkey from a small index. Deterministic, valid by
/// `is_hex_pubkey`. Matches the harness's `test_pubkeys` shape.
fn deterministic_pubkey(idx: u32) -> String {
    let mut hex = String::with_capacity(64);
    for _ in 0..56 {
        hex.push('0');
    }
    hex.push_str(&format!("{idx:08x}"));
    hex
}

/// T114b core invariant: per-pubkey claim consumer-id set is bounded.
/// Pump 4× the cap of unique consumer_ids onto one pubkey. The set must
/// stabilise at `MAX_CLAIMS_PER_PUBKEY` and `claim_drops_total` must record
/// the overflow exactly (4×cap claims sent → cap retained → 3×cap drops).
#[test]
fn claim_profile_set_bounded_at_per_pubkey_cap() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk = deterministic_pubkey(0);

    let n = MAX_CLAIMS_PER_PUBKEY * 4;
    for i in 0..n {
        // Unique consumer_id per call — mirrors S2's mix (no matching release).
        kernel.claim_profile(pk.clone(), format!("c{i}"), false, false);
    }

    assert_eq!(
        kernel.profile_claims_len_for_test(&pk),
        MAX_CLAIMS_PER_PUBKEY,
        "claim set must stabilise at cap"
    );
    assert_eq!(
        kernel.claim_drops_total_test(),
        (n - MAX_CLAIMS_PER_PUBKEY) as u64,
        "every overflow must be counted"
    );
}

/// T114b — D6 invariant: a dropped claim is a silent no-op, not an FFI error.
/// `claim_profile` returns `Vec<OutboundMessage>` for the actor's outbound
/// path; a dropped claim must produce an empty Vec, never a panic or partial
/// mutation that could later trip an assertion.
#[test]
fn dropped_claim_is_silent_noop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk = deterministic_pubkey(0);

    // Fill to cap.
    for i in 0..MAX_CLAIMS_PER_PUBKEY {
        kernel.claim_profile(pk.clone(), format!("c{i}"), false, false);
    }
    assert_eq!(kernel.claim_drops_total_test(), 0);

    // One past the cap.
    let overflow = kernel.claim_profile(pk.clone(), "overflow-consumer".into(), false, false);
    assert!(
        overflow.is_empty(),
        "dropped claim must return empty outbound"
    );
    assert_eq!(kernel.claim_drops_total_test(), 1);

    // Re-claiming an already-present consumer is NOT a drop — it's an
    // idempotent no-op handled by `BTreeSet::insert` returning false. The
    // cap check must skip when the consumer is already in the set.
    let dup = kernel.claim_profile(pk.clone(), "c0".into(), false, false);
    assert!(dup.is_empty());
    assert_eq!(
        kernel.claim_drops_total_test(),
        1,
        "duplicate claim of existing consumer must NOT count as drop"
    );
}

/// T114b — distinct pubkeys retain independent caps. Filling one pubkey's
/// set to cap must not steal capacity from another pubkey.
#[test]
fn claim_cap_is_per_pubkey_not_global() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk_a = deterministic_pubkey(1);
    let pk_b = deterministic_pubkey(2);

    // Saturate pk_a.
    for i in 0..(MAX_CLAIMS_PER_PUBKEY + 16) {
        kernel.claim_profile(pk_a.clone(), format!("a{i}"), false, false);
    }
    assert_eq!(
        kernel.profile_claims_len_for_test(&pk_a),
        MAX_CLAIMS_PER_PUBKEY
    );
    assert_eq!(kernel.claim_drops_total_test(), 16);

    // pk_b is fresh — claims must succeed up to its own cap.
    for i in 0..32 {
        kernel.claim_profile(pk_b.clone(), format!("b{i}"), false, false);
    }
    assert_eq!(kernel.profile_claims_len_for_test(&pk_b), 32);
    assert_eq!(
        kernel.claim_drops_total_test(),
        16,
        "filling pk_b must not bump the global drop counter beyond pk_a's overflow"
    );
}

/// T114b — release path is still effective after a drop episode. Once
/// existing consumers release, freed slots accept new claims again. This
/// pins the recovery semantic: drop-newest is not a permanent block.
#[test]
fn claim_recovers_after_release_post_drop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk = deterministic_pubkey(3);

    // Fill + overflow.
    for i in 0..(MAX_CLAIMS_PER_PUBKEY + 1) {
        kernel.claim_profile(pk.clone(), format!("c{i}"), false, false);
    }
    assert_eq!(kernel.claim_drops_total_test(), 1);

    // Release one existing consumer (c0..c1023 are in the set; the overflow
    // c1024 was dropped, so releasing c0 frees a slot).
    kernel.release_profile(&pk, "c0");
    assert_eq!(
        kernel.profile_claims_len_for_test(&pk),
        MAX_CLAIMS_PER_PUBKEY - 1
    );

    // The previously-dropped consumer can now claim.
    kernel.claim_profile(pk.clone(), "post-release-consumer".into(), false, false);
    assert_eq!(
        kernel.profile_claims_len_for_test(&pk),
        MAX_CLAIMS_PER_PUBKEY
    );
    assert_eq!(
        kernel.claim_drops_total_test(),
        1,
        "post-release claim must NOT bump drops (slot was free)"
    );
}

/// T114b — the FFI-channel drop counter (`dispatch_drops`) round-trips
/// through the kernel snapshot. This pins the surface advisor flagged: the
/// counter must not be `let _ = …`'d into oblivion — it has to reach the
/// diagnostic surface so operators can observe FFI-channel pressure.
#[test]
fn dispatch_drops_handle_surfaces_on_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert_eq!(
        kernel.dispatch_drops_total(),
        0,
        "unbound kernel reports zero"
    );

    let handle = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_dispatch_drops_handle(std::sync::Arc::clone(&handle));

    // External mutation (mirrors the FFI forwarder thread incrementing on Full).
    handle.fetch_add(42, Ordering::Relaxed);
    assert_eq!(
        kernel.dispatch_drops_total(),
        42,
        "kernel must observe external Arc increments"
    );

    // Reset round-trip: extract → reinstall onto fresh kernel.
    let extracted = kernel.take_dispatch_drops_handle_for_reset();
    assert!(extracted.is_some());
    let mut fresh = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    fresh.set_dispatch_drops_handle(extracted.unwrap());
    handle.fetch_add(1, Ordering::Relaxed);
    assert_eq!(
        fresh.dispatch_drops_total(),
        43,
        "Reset must preserve counter via take→set round-trip"
    );
}

/// G-S4 — the actor command-channel depth counter (`queue_depth`)
/// round-trips through the kernel snapshot accessor. Mirrors
/// `dispatch_drops_handle_surfaces_on_kernel`: the `Arc<AtomicU64>` shared
/// with `NmpApp::send_cmd` must reach `actor_queue_depth()` so the snapshot
/// surfaces real command-channel occupancy, and must survive `Reset`.
#[test]
fn queue_depth_handle_surfaces_on_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert_eq!(
        kernel.actor_queue_depth(),
        0,
        "unbound kernel reports zero queue depth"
    );

    let handle = std::sync::Arc::new(AtomicU64::new(0));
    kernel.set_queue_depth_handle(std::sync::Arc::clone(&handle));

    // External mutation (mirrors `NmpApp::send_cmd` incrementing on send).
    handle.fetch_add(7, Ordering::Relaxed);
    assert_eq!(
        kernel.actor_queue_depth(),
        7,
        "kernel must observe external Arc increments"
    );

    // Saturation: a count above u32::MAX collapses to u32::MAX, not a wrap.
    handle.store(u64::from(u32::MAX) + 100, Ordering::Relaxed);
    assert_eq!(
        kernel.actor_queue_depth(),
        u32::MAX,
        "queue depth saturates at u32::MAX"
    );
    handle.store(7, Ordering::Relaxed);

    // Reset round-trip: extract → reinstall onto fresh kernel.
    let extracted = kernel.take_queue_depth_handle_for_reset();
    assert!(extracted.is_some());
    let mut fresh = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    fresh.set_queue_depth_handle(extracted.unwrap());
    handle.fetch_add(1, Ordering::Relaxed);
    assert_eq!(
        fresh.actor_queue_depth(),
        8,
        "Reset must preserve the queue-depth counter via take→set round-trip"
    );
}

/// T114b — allocation-bounded harness using the global allocator. Pumps
/// 16× MAX_CLAIMS_PER_PUBKEY claims (16k unique consumer_ids) onto one
/// pubkey and asserts that the bound is observed via the public counter +
/// the set size. This is a deterministic functional check; the
/// counting-allocator NET-heap retention proof lives in
/// `crates/nmp-testing/bin/ffi-stress/s2_dispatch_flood.rs` (the binary
/// owns `#[global_allocator]` so the lib-test can't reuse that path).
#[test]
fn claim_flood_does_not_grow_unbounded() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk = deterministic_pubkey(7);

    let flood_size = MAX_CLAIMS_PER_PUBKEY * 16;
    for i in 0..flood_size {
        kernel.claim_profile(pk.clone(), format!("flood-{i:08}"), false, false);
    }

    // The set is at cap, no more.
    assert_eq!(
        kernel.profile_claims_len_for_test(&pk),
        MAX_CLAIMS_PER_PUBKEY,
        "16× flood must NOT grow past cap"
    );

    // Drops counter recorded every overflow.
    assert_eq!(
        kernel.claim_drops_total_test(),
        (flood_size - MAX_CLAIMS_PER_PUBKEY) as u64,
        "every overflow accounted for"
    );

    // Memory bound proof: if the BTreeSet were still growing per-dispatch
    // (a pre-fix regression), `len()` would be `flood_size` not the cap.
    // The set's heap footprint is therefore O(MAX_CLAIMS_PER_PUBKEY × avg
    // consumer_id size), independent of dispatch count — the D8 invariant.
}

// ── T133: wire_subs row eviction ─────────────────────────────────────────────

// V-112 (ADR-0042): `view_close_evicts_wire_subs_to_zero` deleted.
// That test called `kernel.open_author()` / `kernel.close_author()` (both
// deleted). T133 view-close eviction is now exercised at the FFI layer via
// `nmp_app_open_interest` / `nmp_app_close_interest`; the oneshot-EOSE and
// CLOSED-frame paths below remain as the primary kernel-level T133 pins.

/// T133 — EOSE for a non-keep sub (oneshot: profile-claim, author-profile,
/// thread-ids, …) evicts the row from `wire_subs`. This is the
/// higher-volume retention source than view-close: every claim and every
/// thread hydration ends via EOSE.
#[test]
fn eose_evicts_wire_sub_row() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Stage a wire-sub via the same insertion path the production code uses.
    let req = kernel.req_for_relay(
        RelayRole::Indexer,
        "wss://relay.test".to_string(),
        "profile-claim-1-abcd1234",
        "T133 eviction probe",
        serde_json::json!({"kinds":[0],"authors":["aa".repeat(32)],"limit":1}),
    );
    assert_eq!(req.text.split("\"REQ\"").count(), 2, "one REQ emitted");
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        1,
        "REQ inserted exactly one row"
    );

    // Simulate the relay's EOSE — the kernel must (a) emit a CLOSE outbound
    // and (b) evict the row.
    let frame = serde_json::json!(["EOSE", "profile-claim-1-abcd1234"]).to_string();
    let outbound = kernel.handle_text(RelayRole::Indexer, "wss://relay.test", &frame);
    assert!(
        outbound
            .iter()
            .any(|m| m.text.contains("CLOSE") && m.text.contains("profile-claim-1-abcd1234")),
        "EOSE for a oneshot must emit a CLOSE outbound"
    );
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "EOSE for a oneshot must evict the wire_subs row"
    );
}

/// T133 — relay-initiated CLOSED frame evicts the row outright (no outbound
/// CLOSE — the relay already declared the sub dead).
#[test]
fn closed_frame_evicts_wire_sub_row() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    kernel.req_for_relay(
        RelayRole::Content,
        "wss://relay.test".to_string(),
        "author-notes-7-deadbeef",
        "T133 CLOSED-frame eviction probe",
        serde_json::json!({"kinds":[1,6],"authors":["bb".repeat(32)],"limit":100}),
    );
    assert_eq!(kernel.wire_subs_len_for_test(), 1);

    let frame =
        serde_json::json!(["CLOSED", "author-notes-7-deadbeef", "rate-limited"]).to_string();
    let _ = kernel.handle_text(RelayRole::Content, "wss://relay.test", &frame);
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "CLOSED frame must evict the row"
    );
}

/// T133 — `relay_closed` (per-URL socket teardown) evicts every row for the
/// closed socket's URL; rows on a different URL are preserved. `relay_failed`
/// (transient → state="retrying") does NOT evict — the sub may resume after
/// the backoff window.
#[test]
fn relay_closed_evicts_per_url_relay_failed_preserves() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Two subs on the Indexer lane, one on Content.
    kernel.req_for_relay(
        RelayRole::Indexer,
        "wss://idx.test".to_string(),
        "profile-claim-1-aaaa",
        "ix probe a",
        serde_json::json!({"kinds":[0]}),
    );
    kernel.req_for_relay(
        RelayRole::Indexer,
        "wss://idx.test".to_string(),
        "profile-claim-2-bbbb",
        "ix probe b",
        serde_json::json!({"kinds":[0]}),
    );
    kernel.req_for_relay(
        RelayRole::Content,
        "wss://content.test".to_string(),
        "author-notes-1-cccc",
        "content probe",
        serde_json::json!({"kinds":[1]}),
    );
    assert_eq!(kernel.wire_subs_len_for_test(), 3);

    // relay_failed must NOT evict — it only marks "retrying".
    kernel.relay_failed(
        RelayRole::Indexer,
        "wss://idx.test",
        "transient error".to_string(),
    );
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        3,
        "relay_failed is transient — rows preserved"
    );

    // relay_closed evicts every row on that URL.
    kernel.relay_closed(RelayRole::Indexer, "wss://idx.test");
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        1,
        "relay_closed evicts the two idx.test rows; Content row preserved"
    );

    // Content socket still healthy.
    kernel.relay_closed(RelayRole::Content, "wss://content.test");
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "relay_closed on the content URL evicts the last row"
    );
}

/// T105 regression — under URL-keyed routing several sockets share one
/// `RelayRole` lane. Closing ONE socket must evict only that socket's
/// wire-subs; a sibling socket on the *same role* must keep its subscriptions
/// live. A role-wide `retain` (the pre-fix behaviour) would silently drop the
/// healthy sibling's REQs.
#[test]
fn relay_closed_does_not_evict_sibling_url_on_same_role() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Two Content-lane sockets — different URLs, same RelayRole.
    kernel.req_for_relay(
        RelayRole::Content,
        "wss://content-a.test".to_string(),
        "author-notes-1-aaaa",
        "content A",
        serde_json::json!({"kinds":[1]}),
    );
    kernel.req_for_relay(
        RelayRole::Content,
        "wss://content-b.test".to_string(),
        "author-notes-2-bbbb",
        "content B",
        serde_json::json!({"kinds":[1]}),
    );
    assert_eq!(kernel.wire_subs_len_for_test(), 2);

    // Close ONLY content-a — content-b shares the role but is a live socket.
    kernel.relay_closed(RelayRole::Content, "wss://content-a.test");
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        1,
        "closing content-a must NOT evict content-b's sub (same role, live socket)"
    );
    let surviving = kernel.snapshot_active_wire_subs();
    assert!(
        surviving.iter().any(|(_id, url)| url.contains("content-b")),
        "the sibling socket's wire-sub must survive; got {surviving:?}"
    );

    // relay_failed is likewise URL-scoped: failing content-b marks only
    // content-b and never evicts (the sub may resume after backoff).
    kernel.relay_failed(
        RelayRole::Content,
        "wss://content-b.test",
        "transient".to_string(),
    );
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        1,
        "relay_failed must not evict — content-b's row stays (now retrying)"
    );

    // The full teardown path still clears the whole lane.
    kernel.relay_closed_all(RelayRole::Content);
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "relay_closed_all evicts every row on the role lane"
    );
}
