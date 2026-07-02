//! PR-2 rawtap retirement: cache-serve feeds kind:1059 via IngestParser only.

use super::universal_fixtures_support::{gift_wrap_json, register_one, CapturingIngestParser};
use crate::kernel::cache_serve_tests::{drain_cache_serves, simulate_cold_restart};
use crate::kernel::Kernel;
use crate::planner::InterestShape;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::SubKey;
use nmp_network::role::RelayRole;
use std::collections::BTreeSet;

#[test]
fn e2_cache_serve_feeds_ingest_parser_for_kind_1059() {
    let base_ts: u64 = 1_700_000_000;
    let receiver_keys = ::nostr::Keys::generate();
    let receiver_hex = receiver_keys.public_key().to_hex();
    let sender_keys = ::nostr::Keys::generate();

    // ── Kernel with wired IngestParser for kind:1059 ──────────────────────────
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Wire the capturing IngestParser into the kernel's shared dispatcher slot.
    let ingest_parser = CapturingIngestParser::new();
    kernel.register_ingest_parser(1059, ingest_parser.clone());

    kernel.active_account = Some(receiver_hex.clone());

    // ── Phase 1: seed a kind:1059 gift-wrap into the store ────────────────────
    let (gift_wrap_json_val, gift_wrap_id) = gift_wrap_json(
        &sender_keys,
        &receiver_keys.public_key(),
        "parser cache-serve test",
        base_ts,
    );
    kernel.handle_event(
        RelayRole::Content,
        "wss://relay.test/",
        "dm",
        &gift_wrap_json_val,
    );

    // Confirm the parser received it on ingest (live path).
    assert!(
        ingest_parser.seen().contains(&1059),
        "Phase 1: IngestParser must see kind:1059 on live ingest"
    );

    // ── Phase 2: cold restart — clear caches + reset counters ─────────────────
    simulate_cold_restart(&mut kernel);
    ingest_parser.clear();

    assert!(
        ingest_parser.seen().is_empty(),
        "Phase 2: IngestParser seen list must be cleared before cache-serve"
    );

    // ── Phase 3: register interest and drain cache-serve for kind:1059 ──────────
    {
        let mut dm_shape = InterestShape {
            kinds: BTreeSet::from([1059u32]),
            ..Default::default()
        };
        dm_shape
            .tags
            .insert("p".to_string(), BTreeSet::from([receiver_hex.clone()]));
        let dm_key = SubKey::new(("ingest-parser-test", &receiver_hex));
        register_one(
            &mut kernel,
            "test-ingest-parser",
            dm_key,
            dm_shape,
            "test-ingest-parser-dm",
        );
    }
    drain_cache_serves(&mut kernel, 10);

    // ── Phase 4: IngestParser must receive kind:1059 from cache-serve ─────────
    // No raw-observer assertion: raw-tap PR-2 removed the dual fan-out from
    // cache-serve. Cache-serve now delivers exclusively via IngestParser.
    let ingest_seen = ingest_parser.seen();
    assert!(
        ingest_seen.contains(&1059),
        "E2/PR-2 FAIL: IngestParser must receive kind:1059 after cold-restart cache-serve; \
         got {ingest_seen:?} — the DmInboxProjection / MarmotIngestParser would not decrypt \
         after restart"
    );

    assert!(
        kernel.events.contains_key(gift_wrap_id.as_str()),
        "E2/PR-2 FAIL: gift-wrap must be in events cache after cache-serve"
    );
}
