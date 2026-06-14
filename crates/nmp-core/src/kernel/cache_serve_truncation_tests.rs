//! K3 Stage B3 / #1380 — Etag/Ptag truncated-serve floor-refusal tests.
//!
//! Split from `cache_serve_watermark_tests.rs` for the 500-LOC file ceiling
//! (AGENTS.md file-size rule). Covers the cursor-less (`Etag`/`Ptag`)
//! budget-truncation → watermark-floor-refusal seam (Stage B3) and the #1380
//! SubKey-aware truncation-set identity (Bug 1), so one interest's exhaustion
//! cannot clear a sibling interest's still-active mark.
//!
//! Shared fixtures live in `cache_serve_tests.rs` as `pub(super)` helpers.

use super::cache_serve::shape_to_store_queries;
use super::cache_serve_tests::{hex_pk, register_interest_for_test, seed_events};
use super::*;
use crate::planner::InterestShape;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::StoreQuery;
use std::collections::BTreeSet;

impl Kernel {
    /// Whether a `completion_key` currently carries a cursor-less truncation
    /// mark — test-only window into `etag_ptag_truncated_serves` (the
    /// SubKey-aware write surface) for the #1380 two-interest regression.
    fn is_completion_key_truncated_for_test(&self, completion_key: u64) -> bool {
        self.etag_ptag_truncated_serves
            .lock()
            .map(|set| set.contains(&completion_key))
            .unwrap_or(false)
    }
}

// ─── K3 Stage B3: Etag/Ptag truncated-serve floor refusal ───────────────────

/// Insert one kind:1 event that `#e`-tags `target_hex` into the store (the
/// thread-reply shape the `Etag` query indexes). Uses the same store-handle
/// insert path the K3 nip77 fixtures use, so the event is real index data.
fn insert_etag_event(kernel: &mut Kernel, id_hex: &str, target_hex: &str, created_at: u64) {
    use crate::store::{RawEvent, VerifiedEvent};
    let raw = RawEvent {
        id: id_hex.to_string(),
        pubkey: hex_pk("aa"),
        created_at,
        kind: 1,
        tags: vec![vec!["e".to_string(), target_hex.to_string()]],
        content: String::new(),
        sig: "a".repeat(128),
    };
    kernel
        .event_store_handle()
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://cache.example".to_string(),
            created_at.saturating_mul(1_000),
        )
        .expect("etag event insert");
}

/// Build the `#e` thread-reply interest shape for `target_hex`.
fn etag_thread_shape(target_hex: &str) -> InterestShape {
    let mut shape = InterestShape {
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    shape
        .tags
        .insert("e".to_string(), BTreeSet::from([target_hex.to_string()]));
    shape
}

/// K3 Stage B3 ORACLE — a budget-truncated `Etag` serve does NOT floor that
/// shape's REQ.
///
/// `Etag`/`Ptag` store-serve queries carry no resume cursor. When the per-tick
/// budget is exhausted mid-serve the chunk advances PAST the query, silently
/// skipping the stored tail *within serve depth*. Since ADR-0045 E2/E3 enabled
/// the watermark floor for these shapes, the relay would then never re-send the
/// skipped stored events that session — a permanent hole. The fix records the
/// budget-truncation and the watermark refuses to floor the shape, so the relay
/// re-sends the gap.
///
/// Orchestration (deterministic, no clock/relay): `visible_limit = 4`, so the
/// aggregate tick budget is `2 × 4 = 8`. Two author serves are capped at
/// `limit = 3` (so each serves depth 3 and consumes 3 of the budget), and the
/// `Etag` serve is reached with only `8 − 3 − 3 = 2` budget left:
///
/// - Serve A (author, `limit = 3`, 3 stored kind:1) → consumes 3 budget.
/// - Serve B (author, `limit = 3`, 3 stored kind:1) → consumes 3 budget.
///   Budget left: 2.
/// - Serve C (the `Etag` thread shape, 6 stored matches, depth 4) → its
///   `visit_limit = min(tick_remaining = 2, remaining_depth = 4) = 2`, so it
///   visits 2 (NOT exhausted, 6 exist) and `remaining_depth = 2 > 0` →
///   BUDGET-truncated within depth. The cursor-less branch records the
///   truncation.
///
/// The in-memory `events`/`timeline` caches are cleared after seeding so the
/// aggregate-window floor (which only engages once the timeline is full) and
/// live→serve dedup do not perturb the per-serve visit budget.
///
/// The watermark for the `Etag` shape must therefore be `None` (no floor).
///
/// FAILS on pre-B3 master (the floor is the newest stored Etag match's
/// timestamp), passes after B3's truncated-serve refusal lands.
#[test]
fn budget_truncated_etag_serve_is_not_floored() {
    use super::cache_serve::completion_key_for_interest;
    use super::cache_serve_tests::simulate_cold_restart;

    let mut kernel = Kernel::new(4); // visible_limit = 4 → budget 8
    let base_ts: u64 = 1_700_000_000;

    // ── Serve A: author capped at limit 3, 3 stored kind:1 (consumes 3) ──────
    let keys_a = ::nostr::Keys::generate();
    let author_a = keys_a.public_key().to_hex();
    kernel.timeline_authors.insert(author_a.clone());
    seed_events(&mut kernel, &keys_a, 3, base_ts);
    let shape_a = InterestShape {
        authors: BTreeSet::from([author_a.clone()]),
        kinds: BTreeSet::from([1u32]),
        limit: Some(3),
        ..Default::default()
    };

    // ── Serve B: a second author, same shape (consumes 3) ────────────────────
    let keys_b = ::nostr::Keys::generate();
    let author_b = keys_b.public_key().to_hex();
    kernel.timeline_authors.insert(author_b.clone());
    seed_events(&mut kernel, &keys_b, 3, base_ts + 100);
    let shape_b = InterestShape {
        authors: BTreeSet::from([author_b.clone()]),
        kinds: BTreeSet::from([1u32]),
        limit: Some(3),
        ..Default::default()
    };

    // ── Serve C: the Etag thread shape with 6 stored matches (depth 4) ───────
    let target_hex = hex_pk("e7");
    for i in 0..6u64 {
        insert_etag_event(
            &mut kernel,
            &hex_pk(&format!("c{i}")),
            &target_hex,
            base_ts + 200 + i,
        );
    }
    let shape_c = etag_thread_shape(&target_hex);

    // Sanity: the Etag shape maps to exactly one (cursor-less) Etag query.
    assert!(
        matches!(
            shape_to_store_queries(&shape_c).as_slice(),
            [StoreQuery::Etag { .. }]
        ),
        "Etag thread shape must map to one Etag query"
    );

    // Drop the in-memory caches + any serves the seeding ingest queued so the
    // queue order below is exactly A, B, C with no dedup/aggregate-floor skew.
    simulate_cold_restart(&mut kernel);

    // #1380: the truncation read view resolves marks through the live registry,
    // so the truncated Etag interest C must be registered under the SAME SubKey
    // its serve's completion key is derived from. Register it WITHOUT a
    // synchronous drain (the bare `enqueue_cache_serve` below preserves the
    // A,B,C budget orchestration this test depends on).
    let sub_key_c = crate::subs::SubKey::new(3);
    register_interest_for_test(&mut kernel, sub_key_c, &shape_c);

    // Enqueue in order A, B, C (FIFO), then drain ONE aggregate-budget tick.
    let key_a = completion_key_for_interest(&crate::subs::SubKey::new(1), &shape_a);
    let key_b = completion_key_for_interest(&crate::subs::SubKey::new(2), &shape_b);
    let key_c = completion_key_for_interest(&sub_key_c, &shape_c);
    kernel.enqueue_cache_serve(&shape_a, key_a);
    kernel.enqueue_cache_serve(&shape_b, key_b);
    kernel.enqueue_cache_serve(&shape_c, key_c);
    kernel.run_cache_serve_step();

    // The Etag serve was budget-truncated within depth, so the watermark MUST
    // refuse to floor it (else the relay never re-sends the stranded tail).
    let floor = kernel.lifecycle.watermark_for_shape_for_test(&shape_c);
    assert_eq!(
        floor, None,
        "a budget-truncated Etag serve must NOT floor its REQ — the stored tail \
         was stranded within serve depth and the relay must re-send it (got a \
         floor of {floor:?})"
    );
}

/// B3 companion — a NON-truncated (fully-served) Etag shape IS still floored,
/// so the refusal is scoped to the truncation hazard and does not blanket-
/// disable the floor for thread shapes. A single Etag serve whose match count
/// is within both the budget AND serve depth completes without truncation, and
/// the watermark floors it at the newest stored match.
#[test]
fn fully_served_etag_shape_is_still_floored() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let base_ts: u64 = 1_700_000_000;
    let target_hex = hex_pk("e8");
    // 2 stored matches — well within budget and depth, so no truncation.
    insert_etag_event(&mut kernel, &hex_pk("d0"), &target_hex, base_ts);
    insert_etag_event(&mut kernel, &hex_pk("d1"), &target_hex, base_ts + 1);
    let shape = etag_thread_shape(&target_hex);

    kernel.clear_served_interest_shapes();
    kernel.enqueue_cache_serve(&shape, 0xC0DE);
    kernel.run_cache_serve_step();

    let floor = kernel.lifecycle.watermark_for_shape_for_test(&shape);
    assert_eq!(
        floor,
        Some(base_ts + 1),
        "a fully-served Etag shape (no truncation) must still be floored at its \
         newest stored match — B3 refuses the floor only for budget-truncated \
         serves"
    );
}

/// #1380 Bug 1 — the truncation set is SubKey-aware: one interest's natural
/// exhaustion (REMOVE) must NOT clear a *different* interest's still-active
/// truncation mark, even when the two share the same Etag shape (and therefore
/// the same `cursor_less_query_key`).
///
/// The bug: the truncation set was keyed by `cursor_less_query_key` (shape
/// content only). Two interests A and B with the same Etag shape but different
/// `SubKey`s map to the SAME query key. The serve dedups them separately (their
/// `completion_key`s differ), so both produce distinct `PendingCacheServe`s.
/// When A's serve truncates it INSERTS the shared key; when B's serve later
/// exhausts naturally it REMOVES the shared key — erasing A's still-active mark.
/// The watermark then floors the merged REQ, suppressing the relay re-send of
/// A's stranded below-floor tail → permanent floor-coherence hole (silent
/// missing thread replies / DMs).
///
/// Fix: key by `completion_key` (SubKey-aware). The read view
/// (`etag_ptag_truncated_query_keys`) holds the shared query key iff AT LEAST
/// ONE active interest mapping to it is truncated, so B's exhaustion (which
/// clears only B's completion key) leaves A's mark — and the refused floor —
/// intact.
///
/// Orchestration (deterministic, no clock/relay): same Etag shape, two SubKeys.
/// - Serve A truncates within depth (tight budget) → INSERTs A's mark.
/// - Serve B exhausts naturally (ample budget) → REMOVEs B's mark only.
/// Assert: A's mark survives B's removal AND the merged floor stays refused.
///
/// FAILS on the shape-keyed (pre-#1380) set: B's exhaustion removes the shared
/// key, A's mark is gone, the floor is applied (`Some(_)`). Passes once the set
/// is keyed by `completion_key`.
#[test]
fn truncated_interest_mark_survives_sibling_exhaustion_same_etag_shape() {
    use super::cache_serve::completion_key_for_interest;

    // visible_limit = 4 → aggregate tick budget = 8.
    let mut kernel = Kernel::new(4);
    let base_ts: u64 = 1_700_000_000;

    // One Etag thread target with 3 stored matches. Serve depth = visible_limit
    // = 4 ≥ 3, so a serve with ample budget EXHAUSTS naturally (visits all 3);
    // a serve with budget < 3 is BUDGET-truncated within depth.
    let target_hex = hex_pk("ef");
    for i in 0..3u64 {
        insert_etag_event(
            &mut kernel,
            &hex_pk(&format!("f{i}")),
            &target_hex,
            base_ts + i,
        );
    }
    let shape = etag_thread_shape(&target_hex);

    // A budget-padding author serve: consumes 6 of the 8-event tick budget so
    // the Etag serve that follows is cut by BUDGET (not depth), the truncation
    // condition the cursor-less branch records.
    let keys_pad = ::nostr::Keys::generate();
    let author_pad = keys_pad.public_key().to_hex();
    kernel.timeline_authors.insert(author_pad.clone());
    seed_events(&mut kernel, &keys_pad, 6, base_ts + 1_000);
    let shape_pad = InterestShape {
        authors: BTreeSet::from([author_pad.clone()]),
        kinds: BTreeSet::from([1u32]),
        limit: Some(6),
        ..Default::default()
    };
    let key_pad = completion_key_for_interest(&crate::subs::SubKey::new(900), &shape_pad);

    // Two interests, SAME Etag shape, DIFFERENT SubKeys → distinct completion
    // keys but identical `cursor_less_query_key`. Register both in the live
    // registry so the read-view recompute can resolve each mark.
    let sub_key_a = crate::subs::SubKey::new(101);
    let sub_key_b = crate::subs::SubKey::new(202);
    let key_a = completion_key_for_interest(&sub_key_a, &shape);
    let key_b = completion_key_for_interest(&sub_key_b, &shape);
    assert_ne!(
        key_a, key_b,
        "two SubKeys over the same shape must yield distinct completion keys"
    );
    register_interest_for_test(&mut kernel, sub_key_a, &shape);
    register_interest_for_test(&mut kernel, sub_key_b, &shape);

    // Clear any serves the seeding ingest queued so the order is exactly
    // pad, A with no aggregate-floor skew.
    kernel.clear_served_interest_shapes();
    register_interest_for_test(&mut kernel, sub_key_a, &shape);
    register_interest_for_test(&mut kernel, sub_key_b, &shape);

    // ── Phase 1: pad (consumes 6) then A (only 2 budget left, depth 4, 3
    // matches) → A visits 2, remaining_depth 2 > 0, NOT exhausted → BUDGET
    // truncation → A's mark INSERTed under key_a.
    kernel.enqueue_cache_serve(&shape_pad, key_pad);
    kernel.enqueue_cache_serve(&shape, key_a);
    kernel.run_cache_serve_step();

    assert!(
        kernel.is_completion_key_truncated_for_test(key_a),
        "phase 1: serve A must be budget-truncated within depth (its mark recorded)"
    );
    assert_eq!(
        kernel.lifecycle.watermark_for_shape_for_test(&shape),
        None,
        "phase 1: with A truncated, the merged Etag floor must be refused"
    );

    // ── Phase 2: serve B with ample budget so it exhausts naturally. ─────────
    // Drain any leftover pending work, then enqueue ONLY B under a fresh tick.
    // B visits all 3 matches → exhausted → REMOVEs key_b. This must NOT clear
    // A's key_a mark.
    while kernel.has_pending_cache_serves() {
        kernel.run_cache_serve_step();
    }
    kernel.enqueue_cache_serve(&shape, key_b);
    kernel.run_cache_serve_step();

    assert!(
        kernel.is_completion_key_truncated_for_test(key_a),
        "#1380 Bug 1: serve B's natural exhaustion must NOT clear A's truncation \
         mark — they share the Etag shape but are distinct interests (different \
         SubKeys → different completion keys). Pre-fix the shape-keyed REMOVE \
         erased A's mark here."
    );
    assert!(
        !kernel.is_completion_key_truncated_for_test(key_b),
        "serve B exhausted naturally, so its own mark must be cleared"
    );
    assert_eq!(
        kernel.lifecycle.watermark_for_shape_for_test(&shape),
        None,
        "#1380 Bug 1: A is still truncated, so the merged Etag floor must REMAIN \
         refused after B's exhaustion — else the relay never re-sends A's \
         stranded below-floor tail (silent missing replies)."
    );
}
