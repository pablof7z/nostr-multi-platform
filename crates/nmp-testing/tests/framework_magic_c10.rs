//! Framework Magic §C10 — Sync watermarks gate backfill; cache-miss authoritative.
//!
//! Standalone test binary so the C10 wiring proof can grow without breaking
//! the 500 LOC ceiling on `framework_magic_contract.rs` (per AGENTS.md). The
//! contract-table meta-test (`framework_magic_contract::contract_surface_complete`)
//! key off the doc-table column for `c10_…`, not on this file's path —
//! splitting it out is invisible at the contract layer.
//!
//! Design: `docs/design/framework-magic/sync.md` §C10.
//! Codex review: `docs/perf/codex-reviews/076173d.md` (T53 follow-up — wires
//! `apply_coverage_filter` into the actor plan-emit path, flips C10 active).

use std::sync::Arc;

use nmp_core::planner::{
    canonical_filter_hash,
    InMemoryMailboxCache,
    InterestId,
    InterestLifecycle,
    InterestScope,
    InterestShape,
    LogicalInterest,
    MailboxSnapshot,
};
use nmp_core::store::{EventStore, MemEventStore, SyncMethod, WatermarkKey, WatermarkRow};
use nmp_core::subs::{SubscriptionLifecycle, WireFrame};
use nmp_nip77::capability::{CapabilityCache, InMemoryCapabilityCache, RelayCapabilities};
use nmp_nip77::planner_gate::apply_coverage_filter;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn pubkey(seed: &str) -> String {
    format!("{seed:0>64}")
        .chars()
        .take(64)
        .collect::<String>()
        .to_lowercase()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Hash-shape -> 32-byte watermark key.
///
/// The planner gate keys watermarks by `[u8; 32]` (see
/// `WatermarkKey::filter_hash`) but the canonical hash today is the 8-hex
/// `String` `canonical_filter_hash` returns. Expand it deterministically so
/// the gate and the test agree on identity; the real BLAKE3-CBOR encoder
/// (`docs/design/lmdb/watermarks.md` §3) will replace both call-sites in one
/// edit.
fn canon(sub: &nmp_core::planner::SubShape) -> [u8; 32] {
    let hex = canonical_filter_hash(&sub.shape);
    let bytes = hex.as_bytes();
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = bytes[i % bytes.len()];
    }
    out
}

fn canon_shape(shape: &InterestShape) -> [u8; 32] {
    let hex = canonical_filter_hash(shape);
    let bytes = hex.as_bytes();
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = bytes[i % bytes.len()];
    }
    out
}

// ── C10 ──────────────────────────────────────────────────────────────────────

/// C10: Watermarks gate backfill; cache miss becomes authoritative; NIP-77 default.
///
/// Wires the lifecycle's `set_coverage_hook` seam (M4) to
/// `nmp_nip77::apply_coverage_filter` and asserts the four observable
/// properties from `docs/design/framework-magic/sync.md` §C10:
///
/// 1. **Unsynced pair → fetch.** No watermark + no capability data → the
///    planner emits a REQ (the hook keeps the sub-shape).
/// 2. **Fully-synced pair → authoritative miss.** Once a fresh `CompleteAsOf`
///    watermark exists for the `(filter, relay)`, the hook drops the
///    sub-shape and the plan diff CLOSEs the live REQ.
/// 3. **NIP-77 capability is the default backfill.** A stale `PartialUpTo`
///    watermark on a NIP-77-capable relay keeps the sub-shape intact
///    (`SyncStrategy::NegThenReq`) — the actor's NIP-77 sync path handles
///    backfill, the planner does not duplicate it with a since-bumped REQ.
/// 4. **Capability fallback.** A stale watermark on a relay that does NOT
///    support NIP-77 bumps `since` to `synced_up_to + 1`
///    (`SyncStrategy::ReqSince`) AND recomputes the sub-shape's
///    `canonical_filter_hash` so the wire-emitter actually routes the new
///    REQ (P1 plan-identity guarantee from
///    `docs/perf/codex-reviews/076173d.md`).
#[test]
fn c10_watermark_gates_backfill_and_authoritative_miss() {
    // Single author routed to a single relay so the compiler emits exactly
    // one sub-shape we can inspect.
    let author = pubkey("alice-c10");
    let relay_url = "wss://relay.c10.test/".to_string();
    let interest_shape = InterestShape {
        authors: [author.clone()].into_iter().collect(),
        kinds: [1u32].into_iter().collect(),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(910),
        scope: InterestScope::Global,
        shape: interest_shape.clone(),
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    };
    // Since the compiler is deterministic and only one interest+mailbox
    // pair exists, the live sub-shape's canonical hash matches the hash
    // we compute directly from the interest's shape.
    let filter_hash_active = canon_shape(&interest_shape);

    let store = Arc::new(MemEventStore::new());
    let caps = Arc::new(InMemoryCapabilityCache::new());

    let mut lifecycle = SubscriptionLifecycle::new();
    // T132: the lifecycle no longer owns a mailbox cache; the test owns one
    // and passes it through `recompile_and_diff`. In production this is the
    // kernel's `KernelMailboxes` adapter (a borrow of `author_relay_lists`).
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        author.clone(),
        MailboxSnapshot {
            write_relays: vec![relay_url.clone()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let store_for_hook = Arc::clone(&store);
    let caps_for_hook = Arc::clone(&caps);
    lifecycle.set_coverage_hook(Arc::new(move |plan| {
        let _ = apply_coverage_filter(plan, &*store_for_hook, &*caps_for_hook, canon);
    }));
    lifecycle.registry_mut().push(interest);

    // ── Step 1: unsynced + no capability → REQ flies ────────────────────────
    let frames = lifecycle.recompile_and_diff(&mailboxes).expect("compile #1");
    let reqs_step1 = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .count();
    assert_eq!(
        reqs_step1, 1,
        "unsynced cold open must emit exactly one REQ; got {frames:?}"
    );

    // ── Step 2: fresh CompleteAsOf watermark → SkipReq → CLOSE ──────────────
    let now_s = now_unix();
    store
        .write_watermark(WatermarkRow {
            key: WatermarkKey {
                filter_hash: filter_hash_active,
                relay_url: relay_url.clone(),
            },
            synced_up_to: now_s,
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: now_s,
        })
        .unwrap();

    let frames = lifecycle.recompile_and_diff(&mailboxes).expect("compile #2");
    let closes_step2 = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .count();
    assert_eq!(
        closes_step2, 1,
        "authoritative coverage must CLOSE the live REQ; got {frames:?}"
    );

    // ── Step 3: stale watermark + NIP-77 capability → NegThenReq → REQ kept ──
    //
    // `MemEventStore::coverage` uses `now() - updated_at > 300 s` as the
    // staleness threshold, so we backdate `updated_at` an hour into the past
    // to land in `Coverage::PartialUpTo`.
    let stale_synced_up_to = now_s.saturating_sub(3_600);
    let stale_updated_at = stale_synced_up_to;
    store
        .write_watermark(WatermarkRow {
            key: WatermarkKey {
                filter_hash: filter_hash_active,
                relay_url: relay_url.clone(),
            },
            synced_up_to: stale_synced_up_to,
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: stale_updated_at,
        })
        .unwrap();
    caps.set(
        relay_url.as_str(),
        RelayCapabilities { supports_nip77: true },
    );

    let frames = lifecycle.recompile_and_diff(&mailboxes).expect("compile #3");
    let req_jsons_step3: Vec<_> = frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { filter_json, .. } => Some(filter_json.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        req_jsons_step3.len(),
        1,
        "NegThenReq must re-emit the live REQ unchanged; got {frames:?}"
    );
    assert!(
        !req_jsons_step3[0].contains("\"since\":"),
        "NegThenReq must NOT bump since on the REQ; saw {}",
        req_jsons_step3[0]
    );

    // ── Step 4: stale watermark + NO NIP-77 capability → ReqSince → bump ────
    caps.set(
        relay_url.as_str(),
        RelayCapabilities { supports_nip77: false },
    );
    let frames = lifecycle.recompile_and_diff(&mailboxes).expect("compile #4");
    let req_jsons_step4: Vec<_> = frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { filter_json, .. } => Some(filter_json.clone()),
            _ => None,
        })
        .collect();
    let closes_step4 = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .count();
    assert_eq!(
        req_jsons_step4.len(),
        1,
        "capability fallback must emit a since-bumped REQ; got {frames:?}"
    );
    assert_eq!(
        closes_step4, 1,
        "capability fallback must CLOSE the prior (un-bumped) REQ because \
         the sub-id changed — proves P1 hash-recompute reaches the wire"
    );
    let expected_since = stale_synced_up_to + 1;
    assert!(
        req_jsons_step4[0].contains(&format!("\"since\":{expected_since}")),
        "ReqSince must bump since to synced_up_to + 1 ({expected_since}); \
         saw {}",
        req_jsons_step4[0]
    );
}
