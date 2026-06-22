//! ADR-0063 (#1671 Lane B) — shared ref-resolver lifecycle unit tests: per-key
//! rev boundedness (BLOCKING 2), no-op-re-resolve rev gating (BLOCKING 3), the
//! unified event terminal-miss teardown (BLOCKING 1 + 2), and the profile
//! per-consumer Live-owner downgrade (HIGH 5).
//!
//! Split out of `refs_tests_profile.rs` / `refs_tests_event.rs` to keep both
//! under the 500-LOC hard ceiling (AGENTS.md). The pure profile / event resolver
//! tests stay in their files; `refs_tests_event.rs` remains the Lane D merge
//! target. These cross-namespace lifecycle tests live here.

use super::refs::{EventShape, ProfileShape, RefLiveness, RefNamespace, RefShape};
use super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

// ADR-0063 / FFI contract: the Event `resolve_ref` / `release_ref` seam takes a
// RAW key (Lane D) — a hex64 event-id (`hex64(...)`) or a `kind:pubkey:d`
// coordinate (`format!("{kind}:{author}:{d_tag}")`) — NOT a `nostr:` URI. The
// `nevent_uri` / `naddr_uri` helpers were removed when the seam went raw-key.

fn profile_card(kernel: &mut Kernel, pk: &str, consumer: &str, liveness: RefLiveness) {
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.to_string(),
        consumer.to_string(),
        RefShape::Profile(ProfileShape::Card),
        liveness,
        false,
        Vec::new(),
    );
}

// ─── BLOCKING 2: per-key rev map stays bounded ───────────────────────────────

/// A spurious release of a never-claimed key must NOT create a permanent row-rev
/// entry — the map only ever holds keys that were actually claimed.
#[test]
fn spurious_release_of_never_claimed_key_creates_no_row() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("9e7e6");
    kernel.release_ref(RefNamespace::Profile, &pk, "ghost");
    assert_eq!(kernel.ref_row_rev(RefNamespace::Profile, &pk), 0);
    assert!(
        kernel
            .projection_rev_tracker
            .source_versions
            .profile_row_revs
            .is_empty(),
        "a spurious profile release must not grow the rev map (BLOCKING 2 (a))"
    );

    // Raw hex64 event-id key (ADR-0063 / FFI contract). It parses cleanly, so the
    // release reaches the refcount logic and finds no claim (actually_removed =
    // false) — the real BLOCKING 2 (a) path, not a malformed-key early no-op.
    let key = hex64("9e7e7");
    kernel.release_ref(RefNamespace::Event, &key, "ghost");
    assert!(
        kernel
            .projection_rev_tracker
            .source_versions
            .event_row_revs
            .is_empty(),
        "a spurious event release must not grow the rev map (BLOCKING 2 (a))"
    );
}

/// BLOCKING 2 — production churn: resolve+release N distinct keys leaves the
/// per-key rev map EMPTY with NO test-only reap call. The last-release teardown
/// (`clear_profile_row`) bumps the row to its final rev and drops the entry in the
/// SAME call, so the map stays bounded to currently-claimed keys (D8). PRE-FIX
/// this grew unbounded (entries were retained in a `pending_clear` set that only a
/// test-only reap drained, and no production code ever reaped).
#[test]
fn per_key_rev_map_stays_bounded_under_resolve_release_churn() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    for i in 0..20u32 {
        let pk = hex64(&format!("ba6c{i:x}"));
        profile_card(&mut kernel, &pk, "c", RefLiveness::CacheOk);
        // Exactly one live key while held.
        assert_eq!(
            kernel
                .projection_rev_tracker
                .source_versions
                .profile_row_revs
                .len(),
            1,
            "a held claim occupies exactly one rev entry"
        );
        // Last release tears the key down: NO reap call, the map empties itself.
        kernel.release_ref(RefNamespace::Profile, &pk, "c");
        assert_eq!(
            kernel
                .projection_rev_tracker
                .source_versions
                .profile_row_revs
                .len(),
            0,
            "the last-release teardown drops the rev entry in the same call (BLOCKING 2)"
        );
    }
    assert!(
        kernel
            .projection_rev_tracker
            .source_versions
            .profile_row_revs
            .is_empty(),
        "resolve→release churn of N distinct keys leaves the per-key rev map empty \
         with no test-only reap (BLOCKING 2)"
    );
}

/// BLOCKING 2 — after a full teardown emitted the explicit `Cleared` (which resets
/// the host cache entry per ADR-0055 §D1), a re-resolve starts a FRESH row
/// lifetime at rev 1. Monotonicity only has to hold WHILE a row is live between
/// `Changed` and `Cleared`, so a reset-after-clear is sound (and required to keep
/// the map bounded).
#[test]
fn re_resolve_after_teardown_starts_a_fresh_row() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("ab12");
    profile_card(&mut kernel, &pk, "c", RefLiveness::CacheOk);
    kernel.release_ref(RefNamespace::Profile, &pk, "c"); // full teardown → row gone
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk),
        0,
        "the teardown removed the rev entry (reads 0 — the row is gone)"
    );

    // Re-resolve AFTER teardown: a brand-new row lifetime begins at rev 1.
    profile_card(&mut kernel, &pk, "c2", RefLiveness::CacheOk);
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk),
        1,
        "a re-resolve after an explicit Cleared starts a fresh row at rev 1"
    );
}

/// BLOCKING 3 — a no-op re-resolve (same key/consumer/shape/liveness, repeated)
/// is NOT a mutation and must NOT advance the per-key rev. PRE-FIX the resolver
/// unconditionally bumped, so three identical re-resolves advanced the rev twice.
#[test]
fn no_op_re_resolve_does_not_advance_per_key_rev() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("0a0a");

    profile_card(&mut kernel, &pk, "c", RefLiveness::CacheOk);
    let rev_after_first = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(rev_after_first > 0, "the first resolve creates the row");

    // Identical (key, consumer, shape, liveness) re-resolve ×3 — pure no-ops.
    for _ in 0..3 {
        profile_card(&mut kernel, &pk, "c", RefLiveness::CacheOk);
    }
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk),
        rev_after_first,
        "a duplicate identical re-resolve must not advance the per-key rev (BLOCKING 3)"
    );

    // A genuine change (shape widen by a NEW consumer) DOES advance it — proves the
    // gate is not simply frozen.
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "wide".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk) > rev_after_first,
        "a real mutation still advances the rev"
    );
}

/// HIGH 5 — a profile slot with one `Live` and one `CacheOk` consumer is Tailing.
/// Releasing the LAST Live owner while the CacheOk owner remains must DOWNGRADE
/// the slot from Tailing to OneShot in place (no dangling Live tail). PRE-FIX
/// `live_profile_claims` was sticky per-KEY, so the slot stayed Tailing until full
/// teardown.
#[test]
fn profile_live_release_downgrades_slot_while_cacheok_remains() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("d09a");

    // CacheOk consumer + Live consumer share ONE slot → Tailing (Live wins).
    profile_card(&mut kernel, &pk, "feed", RefLiveness::CacheOk);
    profile_card(&mut kernel, &pk, "screen", RefLiveness::Live);
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&pk),
        Some(true),
        "Live + CacheOk on one key is Tailing"
    );

    // Release the LAST Live owner; the CacheOk consumer still holds the key.
    kernel.release_ref(RefNamespace::Profile, &pk, "screen");
    assert!(
        kernel.profile_claims.get(&pk).is_some_and(|c| c.contains("feed")),
        "the CacheOk consumer still holds the refcount"
    );
    assert!(
        !kernel.live_profile_claims.contains_key(&pk),
        "no Live owner remains for the key"
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&pk),
        Some(false),
        "releasing the last Live owner downgrades the surviving slot to OneShot \
         (no dangling Live tail — HIGH 5)"
    );
}

// ─── BLOCKING 1 + 2: unified terminal-miss teardown ──────────────────────────

/// FIX 1 + 2 — a terminal-miss (`Exhausted` / `Budget`: no relay holds the event)
/// must run the SAME unified key teardown as the last-consumer release, leaving NO
/// live `ref_event_shapes` and NO live `event_row_revs` entry for the deleted
/// claim. PRE-FIX the controller removed `event_claims` / `event_claim_requested`
/// directly and left the shape map + per-key rev alive (D4 second-writer — a stale
/// ref row), so both asserts below would fail.
#[test]
fn terminal_miss_runs_unified_teardown_leaving_no_live_shape_or_rev() {
    use super::claim_expansion::ClaimTermination;

    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let author = hex64("dead");
    let d_tag = "doomed";
    let kind = 30023u32;
    let primary_id = format!("{kind}:{author}:{d_tag}");
    // ADR-0063 / FFI contract: the Event `resolve_ref` seam takes the RAW
    // `kind:pubkey:d` coordinate key (Lane D), NOT a `nostr:` naddr URI — the
    // raw coordinate IS `primary_id` here.
    let key = primary_id.clone();

    // A cold CacheOk naddr claim refcounts the key, records its demanded shape, and
    // (via the OneshotApi cold-fetch) registers a claim-expansion tracker.
    kernel.resolve_ref(
        RefNamespace::Event,
        key,
        "embed".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(
        kernel.ref_event_shapes.contains_key(&primary_id),
        "the claim recorded a per-consumer demanded shape"
    );
    assert!(
        kernel.ref_row_rev(RefNamespace::Event, &primary_id) > 0,
        "the claim created a per-key rev row"
    );
    let iid = kernel
        .pending_claims
        .values()
        .find(|c| c.primary_id == primary_id)
        .map(|c| c.interest_id.clone())
        .expect("the cold claim registered a claim-expansion tracker");

    // Drive the controller's terminal-miss path (every relay tried; none had it).
    kernel.terminate_claim(iid, ClaimTermination::Exhausted);

    assert!(
        kernel.event_claims.get(&primary_id).is_none(),
        "terminal-miss drops the refcount row"
    );
    assert!(
        !kernel.ref_event_shapes.contains_key(&primary_id),
        "terminal-miss tears down the per-consumer shape map (no stale ref row)"
    );
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Event, &primary_id),
        0,
        "terminal-miss clears the per-key rev entry (bounded — D8; no second writer)"
    );
    assert!(
        !kernel
            .projection_rev_tracker
            .source_versions
            .event_row_revs
            .contains_key(&primary_id),
        "no event_row_revs entry survives a terminal-miss teardown (BLOCKING 2)"
    );
}

// ─── BLOCKING 3: no-op event re-resolve does not advance the per-key rev ──────

/// FIX 3 (event twin) — a duplicate identical (key, consumer, shape, liveness)
/// event re-resolve is NOT a mutation and must not advance the per-key rev.
/// PRE-FIX the resolver bumped unconditionally.
#[test]
fn event_no_op_re_resolve_does_not_advance_per_key_rev() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let id = hex64("57ab1e");
    // ADR-0063 / FFI contract: the Event `resolve_ref` seam takes the RAW hex64
    // event-id key (Lane D), NOT a `nostr:` nevent URI.
    let key = id.clone();

    kernel.resolve_ref(
        RefNamespace::Event,
        key.clone(),
        "c".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    let rev_after_first = kernel.ref_row_rev(RefNamespace::Event, &id);
    assert!(rev_after_first > 0, "the first resolve creates the row");

    for _ in 0..3 {
        kernel.resolve_ref(
            RefNamespace::Event,
            key.clone(),
            "c".into(),
            RefShape::Event(EventShape::Embed),
            RefLiveness::CacheOk,
            false,
            Vec::new(),
        );
    }
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Event, &id),
        rev_after_first,
        "a duplicate identical event re-resolve must not advance the per-key rev (BLOCKING 3)"
    );
}

// ─── Lane B: released-before-drain parked claim is not resurrected ────────────

/// ADR-0063 (#1671 Lane B, codex re-review) — a hintless event claim that PARKS
/// in `pending_event_claims` (no relay connected) and is then RELEASED before
/// the relay-ready drain runs must NOT be resurrected by
/// `pending_event_claim_requests`.
///
/// PRE-FIX bug: the unified `teardown_event_claim_key` cleaned only the LIVE
/// per-key maps (`event_claims`, `ref_event_shapes`, the `Live` slot, the rev),
/// never the cold-park queue. The drain then replayed the stale `(uri, consumer)`
/// pair UNCONDITIONALLY, re-inserting a refcount row, a fresh rev, and an
/// in-flight discovery REQ for a claim nobody holds — a D4/lifecycle violation.
/// On the pre-fix code this test fails at the `event_claims`/rev/requested
/// assertions below (the released key is resurrected). It passes once release
/// removes the parked stake and the drain skips a released pair.
#[test]
fn parked_event_claim_released_before_drain_is_not_resurrected() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    // 1. NO relay connected → `any_relay_connected()` is false, so the hintless
    //    claim takes the cold-park branch.
    assert!(
        !kernel.any_relay_connected(),
        "precondition: no relay connected so the claim parks"
    );
    let id = hex64("dead10");
    // ADR-0063 / FFI contract: the Event `resolve_ref` seam takes the RAW hex64
    // event-id key (Lane D), NOT a `nostr:` nevent URI. A bare event-id carries
    // no relay hints (the raw seam passes `Vec::new()`), so a cold claim with no
    // relay connected still takes the hintless cold-park branch.
    let key = id.clone();

    // 2. resolve_ref parks the claim: queued in `pending_event_claims`, no live
    //    claim and no registered interest yet (nowhere to send a REQ).
    kernel.resolve_ref(
        RefNamespace::Event,
        key.clone(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(
        kernel.pending_event_claims_len_for_test(),
        1,
        "a hintless cold claim parks in pending_event_claims"
    );
    assert!(
        !kernel.event_claim_is_requested_for_test(&id),
        "a parked claim registers no OneshotApi interest yet"
    );
    assert!(
        !kernel.live_event_claims.contains_key(&id),
        "a CacheOk claim registers no Live tailing slot"
    );

    // 3. release BEFORE any relay connects.
    kernel.release_ref(RefNamespace::Event, &key, "view");
    assert_eq!(
        kernel.pending_event_claims_len_for_test(),
        0,
        "release must drop the consumer's parked stake (primary fix)"
    );

    // 4. connect a relay and run the drain.
    kernel.relay_connected(RelayRole::Content);
    assert!(kernel.any_relay_connected(), "relay is now connected");
    let frames = kernel.pending_event_claim_requests();

    // 5. the released parked claim was NOT resurrected. The no-resurrection
    // PROOF is the kernel-state assertions below — no `event_claims` refcount
    // row, a per-key rev still at 0, no in-flight discovery REQ, and no Live
    // slot. The `frames.is_empty()` check at the end is only a weak corollary
    // (a re-resolve could in principle bump state without emitting a frame, e.g.
    // an already-cached coord), so it is NOT relied on as the resurrection guard.
    assert!(
        kernel.event_claims.get(&id).is_none(),
        "drain must NOT recreate an event_claims refcount row for a released key"
    );
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Event, &id),
        0,
        "drain must NOT bump a fresh per-key rev for a released key"
    );
    assert!(
        !kernel.event_claim_is_requested_for_test(&id),
        "drain must NOT register an in-flight discovery REQ for a released key"
    );
    assert!(
        !kernel.live_event_claims.contains_key(&id),
        "drain must NOT register a Live slot for a released key"
    );
    assert!(
        frames.is_empty(),
        "a fully-released parked claim emits no wire frames on drain: {frames:?}"
    );
}
