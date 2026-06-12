//! Cache-serve regression test: an all-kinds range parser receives kind:1
//! events served from the store.
//!
//! Gap closed by this PR (Finding 3): `shape_needs_ingest_parser_dispatch`
//! only returned true for `#p`+kind:1059 DM shapes, so a KindTime / AuthorKind
//! cache-serve for kind:1 (the follow-feed) would never call
//! `ingest_dispatcher_slot()…dispatch()` for a registered all-kinds range parser
//! (e.g. chirp-tui's `RawCacheIngestParser`, slot `"chirp-tui.raw-cache"`).
//!
//! The architecturally-right fix (owner doctrine: one uniform mechanism):
//! replace the hardcoded shape allowlist with a per-kind registry query
//! (`EventIngestDispatcher::is_interested(kind)`) so ANY registered parser
//! — including future ones — causes cache-serve dispatch without code changes.

use super::cache_serve_tests::{drain_cache_serves, hex_pk, seed_events, simulate_cold_restart};
use super::*;
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::VerifiedEvent;
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use crate::substrate::IngestParser;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

// ─── Fixtures ────────────────────────────────────────────────────────────────

struct CapturingIngestParser {
    seen_kinds: Mutex<Vec<u32>>,
}

impl CapturingIngestParser {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen_kinds: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<u32> {
        self.seen_kinds.lock().unwrap().clone()
    }
}

impl IngestParser for CapturingIngestParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.seen_kinds.lock().unwrap().push(evt.raw().kind);
    }
}

fn sub_id(seed: u64) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(seed),
        SubKey::new(seed),
        SubScope::Global,
    )
}

fn open_author_interest(kernel: &mut Kernel, seed: u64, author_hex: &str) {
    let shape = InterestShape {
        authors: BTreeSet::from([author_hex.to_string()]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let interest = LogicalInterest {
        id: InterestId(seed),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    kernel.open_interest_sub(sub_id(seed), interest);
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// PRIMARY CONTRACT: a cache-served kind:1 event reaches an all-kinds range
/// parser registered on the `EventIngestDispatcher`.
///
/// This guards the chirp-tui `RawCacheIngestParser` (`0..u32::MAX`) use case:
/// the "View raw event" modal must see feed notes that are served from the store
/// on cold restart (not just live-relay notes).
#[test]
fn cache_served_kind1_reaches_all_kinds_ingest_parser() {
    let base_ts: u64 = 1_700_000_100;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // ── Phase 1: seed events into the store via live ingest ───────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.timeline_authors.insert(author.clone());

    // Wire all-kinds parser BEFORE seeding — it should also fire on live ingest.
    let parser = CapturingIngestParser::new();
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.replace_range_parser(
            0..u32::MAX,
            "test.all-kinds",
            Arc::clone(&parser) as Arc<dyn IngestParser>,
        );
    }

    let _ids = seed_events(&mut kernel, &keys, 3, base_ts);
    // Live ingest fires (timeline path + our fix).
    let seen_live = parser.seen();
    assert_eq!(
        seen_live.len(),
        3,
        "all-kinds parser must fire for each kind:1 on live ingest; got {seen_live:?}",
    );

    // ── Phase 2: cold restart (in-memory caches gone, store intact) ───────────
    simulate_cold_restart(&mut kernel);
    // Clear parser state so Phase 3 assertion is unambiguous.
    *parser.seen_kinds.lock().unwrap() = Vec::new();

    // ── Phase 3: open interest and drain cache-serve ───────────────────────────
    open_author_interest(&mut kernel, 10, &author);
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: assert cache-served events reached the all-kinds parser ───────
    let seen_from_store = parser.seen();
    assert_eq!(
        seen_from_store.len(),
        3,
        "all-kinds IngestParser must receive all 3 kind:1 events from cache-serve; \
         got {seen_from_store:?} — chirp-tui RawCacheIngestParser would miss feed notes on cold start",
    );
    assert!(
        seen_from_store.iter().all(|&k| k == 1),
        "all dispatched events must be kind:1; got {seen_from_store:?}",
    );
}

/// DEDUP GATE: an already-in-memory event (live-delivered then cache-served)
/// is skipped by cache-serve and does NOT reach the parser a second time.
///
/// This verifies the existing `events_cache.contains_key` dedup in
/// `serve_chunk` still fires correctly even when the parser is wired.
#[test]
fn cache_served_kind1_no_double_dispatch_when_already_in_memory() {
    let base_ts: u64 = 1_700_000_200;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.timeline_authors.insert(author.clone());

    let parser = CapturingIngestParser::new();
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.replace_range_parser(
            0..u32::MAX,
            "test.all-kinds",
            Arc::clone(&parser) as Arc<dyn IngestParser>,
        );
    }

    // Seed AND leave events in the in-memory cache (no cold restart).
    seed_events(&mut kernel, &keys, 2, base_ts);
    let seen_live = parser.seen().len();
    assert_eq!(seen_live, 2, "live ingest must fire parser twice; got {seen_live}");

    // Reset parser count before the cache-serve run.
    *parser.seen_kinds.lock().unwrap() = Vec::new();

    // Re-open interest (store completion key was cleared by … no restart, so we
    // need to clear it manually to force a re-serve attempt).
    kernel.clear_served_interest_shapes();
    open_author_interest(&mut kernel, 20, &author);
    drain_cache_serves(&mut kernel, 10);

    // Events are ALREADY in the events cache → serve_chunk dedup skips them.
    let seen_from_store = parser.seen();
    assert!(
        seen_from_store.is_empty(),
        "cache-serve must NOT re-dispatch already-in-memory events; got {seen_from_store:?}",
    );
}
