//! Test 4 — negentropy_skips_redundant_req
//!
//! Scenario — watermark `since`-rewrite (T129 / the surviving coverage gate):
//!   1. Build a SubscriptionLifecycle with a WatermarkFn that reports the
//!      local store has events up to ts=1700 for alice's kind:1.
//!   2. Open a tailing interest for alice (no explicit `since`).
//!   3. Compile: assert the emitted REQ carries `"since":1701`
//!      (watermark + 1 — the relay is told to skip events already on disk).
//!   4. No WatermarkFn installed (cold start / empty store): assert no `since`
//!      in the filter (relay sends everything).
//!
//! Design note: this legacy test covers only the T129 watermark-to-`since`
//! rewrite in `SubscriptionLifecycle`; it is not the proof surface for the
//! current `nmp-nip77` runtime.  The rewrite remains a D2-adjacent coverage
//! gate: instead of suppressing the REQ entirely, it narrows it so the relay
//! sends only NEW events.  The rewrite is driven by a `WatermarkFn` installed
//! at kernel construction time (production: `EventStore::query_visit`
//! newest-created_at lookup; tests: any `Arc<dyn Fn(&InterestShape) ->
//! Option<u64>>`).
//!
//! D2 doctrine note: a complete negentropy-driven REQ suppression path would
//! require a shipping coverage hook (`PlanCoverageHook`) that derives its
//! decision from real store state.  The production kernel currently does NOT
//! install such a hook (see `TODO(D2)` in `subs/mod.rs`).  The correct `#[ignore]`
//! for that missing piece lives in `coverage_hook_tests.rs::d2_coverage_hook_slot_round_trips`.
//! This test pins the working, shipping coverage narrowing mechanism instead.

use crate::support::{padded_pubkey, put_write_mailbox, req_filters};

#[test]
fn negentropy_skips_redundant_req() {
    use nmp_core::subs::SubscriptionLifecycle;
    use nmp_planner::{
        InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
        LogicalInterest,
    };
    use std::collections::BTreeSet;
    use std::sync::Arc;

    let mut mailboxes = InMemoryMailboxCache::new();
    put_write_mailbox(&mut mailboxes, padded_pubkey("alice"), "wss://alice-relay/");

    let alice_interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [padded_pubkey("alice")]
                .into_iter()
                .collect::<BTreeSet<_>>(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };

    // Case 1: warm store (watermark=1700) raises an EXISTING since floor → REQ carries since=1701. Per #1281 (T129) a since=None interest is exempt (see Case 2), so this case sets an explicit floor.
    let mut warm_interest = alice_interest.clone();
    warm_interest.shape.since = Some(1);
    let mut lc_warm = SubscriptionLifecycle::new();
    lc_warm.set_watermark_fn(Arc::new(|_shape, _relay: &str| Some(1700)));
    nmp_core::subs::replace_test_interest(&mut lc_warm, warm_interest);
    let frames_warm = lc_warm
        .recompile_and_diff(&mailboxes)
        .expect("warm compile");
    let filters_warm = req_filters(&frames_warm);
    assert!(
        !filters_warm.is_empty(),
        "warm-store compile must still emit a REQ (narrowed, not suppressed)"
    );
    for filter in &filters_warm {
        assert!(
            filter.contains("\"since\":1701"),
            "REQ filter must carry since=watermark+1 to skip already-cached events; \
             got filter: {filter}"
        );
    }

    // Case 2: cold start (no watermark) → since=None stays None → REQ has no since field (full fetch).
    let mut lc_cold = SubscriptionLifecycle::new();
    lc_cold.set_watermark_fn(Arc::new(|_shape, _relay: &str| None));
    nmp_core::subs::replace_test_interest(&mut lc_cold, alice_interest);
    let frames_cold = lc_cold
        .recompile_and_diff(&mailboxes)
        .expect("cold compile");
    let filters_cold = req_filters(&frames_cold);
    assert!(
        !filters_cold.is_empty(),
        "cold-start compile must emit a REQ"
    );
    for filter in &filters_cold {
        assert!(
            !filter.contains("\"since\""),
            "cold-start REQ must have no since field (relay sends everything); \
             got filter: {filter}"
        );
    }
}
