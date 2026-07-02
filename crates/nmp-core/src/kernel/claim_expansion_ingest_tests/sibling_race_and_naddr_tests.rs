//! T-P7 / T-P8 — sibling-relay race guard and naddr (kind:30023) resolution
//! through production wire ingest: a sibling relay's EOSE-no-match must not
//! release a still-in-flight claim whose EVENT arrives from a slower relay,
//! and an addressable kind:30023 article claim resolves via the raw-key
//! event ref seam + production `handle_text(EVENT)` path.

use super::claim_expansion_ingest_support::{event_frame, event_ref_row, eose_frame, signed_article, signed_note};
use crate::kernel::{EventShape, Kernel, RefLiveness, RefNamespace, RefShape};
use crate::refs::RefEventStore;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_network::role::RelayRole;

// ── T-P7: sibling EOSE-no-match must not release a still-in-flight claim ──
#[test]
fn claimed_kind1_surfaces_when_event_arrives_after_sibling_eose_no_match() {
    use crate::kernel::test_support;
    use crate::subs::WireFrame;

    test_support::clear_claim_expansion_subs();

    let relay_a = "wss://sibling-eose.relay"; // EOSEs first, has nothing
    let relay_b = "wss://has-event.relay"; // delivers the EVENT, slower
    let shared_sub_id = "sub-shared-race-0001";

    let keys = ::nostr::Keys::generate();
    let event = signed_note(
        &keys,
        "kind:1 note resolved after sibling EOSE",
        1_700_000_000,
    );
    let primary_id = event.id.clone();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut event_store = RefEventStore::new();

    // Resolve the event through the production raw-key event ref seam: this
    // both refcounts `event_claims[primary_id]` (the key refs.event emits)
    // AND registers the `PendingClaim` controller state. The caller
    // passes the relay hints that used to be carried by the URI TLV.
    let _ = kernel.resolve_ref(
        RefNamespace::Event,
        primary_id.clone(),
        "view-0".to_string(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        vec![relay_a.to_string(), relay_b.to_string()],
    );

    let interest_id = kernel
        .test_claim_interest_id(&primary_id)
        .expect("resolve_ref must register a pending claim with an interest_id");

    // Both relays share the SAME sub_id (same filter shape → same hash).
    let frames = vec![
        WireFrame::Req {
            relay_url: relay_a.to_string(),
            sub_id: shared_sub_id.to_string(),
            filter_json: r#"{"ids":["test"],"limit":1}"#.to_string(),
            interest_id: interest_id.clone(),
            lifecycle: crate::planner::InterestLifecycle::OneShot,
        },
        WireFrame::Req {
            relay_url: relay_b.to_string(),
            sub_id: shared_sub_id.to_string(),
            filter_json: r#"{"ids":["test"],"limit":1}"#.to_string(),
            interest_id,
            lifecycle: crate::planner::InterestLifecycle::OneShot,
        },
    ];
    kernel.register_wire_frames_for_test(&frames);

    // Pre-arrival: the claim row exists but the event has not arrived, so
    // the projection must not surface it yet.
    assert!(
        event_ref_row(&mut kernel, &mut event_store, &primary_id).is_none(),
        "claim row exists but the event has not arrived yet"
    );

    // RACE TRIGGER: relay_a EOSEs WITHOUT the event, through the production
    // EOSE ingest path. This must NOT release the claim — relay_b's EVENT
    // is still in flight.
    kernel.handle_text(RelayRole::Indexer, relay_a, &eose_frame(shared_sub_id));
    assert_eq!(
        kernel.event_claims_len_for_test(&primary_id),
        1,
        "a single relay's EOSE-no-match must NOT release the claim (race guard)"
    );

    // The matching EVENT now arrives from the slower relay_b, through the
    // production EVENT ingest path.
    kernel.handle_text(
        RelayRole::Indexer,
        relay_b,
        &event_frame(shared_sub_id, &event),
    );

    // The claimed kind:1 event MUST now surface in refs.event.
    let entry = event_ref_row(&mut kernel, &mut event_store, &primary_id).expect(
        "claimed kind:1 event must surface in refs.event even when it arrives \
         after a sibling relay's EOSE-no-match",
    );
    assert_eq!(entry.primary_id, primary_id);
    assert_eq!(entry.kind, 1);

    test_support::clear_claim_expansion_subs();
}

// ── T-P8: naddr kind:30023 resolves through production wire ingest ────────
#[test]
fn claimed_naddr_article_surfaces_via_production_wire_ingest() {
    use crate::kernel::test_support;
    use crate::subs::WireFrame;

    test_support::clear_claim_expansion_subs();

    let relay_url = "wss://has-article.relay";
    let shared_sub_id = "sub-naddr-article-0001";
    let d_tag = "the-internet-left-me";

    let keys = ::nostr::Keys::generate();
    let event = signed_article(
        &keys,
        d_tag,
        "What's left of the internet?",
        "long-form body",
        1_700_000_000,
    );
    let author_hex = event.pubkey.clone();
    let coord_key = format!("30023:{author_hex}:{d_tag}");

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut event_store = RefEventStore::new();

    // Resolve through the production raw-key event ref seam. This refcounts
    // `event_claims[coord_key]` (the key refs.event emits) AND registers
    // the W5 PendingClaim. The coordinate key carries the author, so
    // `claim_expansion_match_author` can still admit and score the EVENT.
    let _ = kernel.resolve_ref(
        RefNamespace::Event,
        coord_key.clone(),
        "view-0".to_string(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        vec![relay_url.to_string()],
    );

    let interest_id = kernel
        .test_claim_interest_id(&coord_key)
        .expect("resolve_ref must register a pending claim with an interest_id for the naddr");

    // Register the planner wire frame so `claim_sub_index` is populated and
    // `handle_text(EVENT)` routes the hit to this claim's sub.
    let frames = vec![WireFrame::Req {
        relay_url: relay_url.to_string(),
        sub_id: shared_sub_id.to_string(),
        // The filter body is cosmetic here — `register_wire_frames_for_test`
        // indexes the claim by `sub_id`, it does not re-parse the filter.
        filter_json: r#"{"kinds":[30023],"limit":1}"#.to_string(),
        interest_id,
        lifecycle: crate::planner::InterestLifecycle::OneShot,
    }];
    kernel.register_wire_frames_for_test(&frames);

    // Pre-arrival: claim row exists, event not yet ingested → absent.
    assert!(
        event_ref_row(&mut kernel, &mut event_store, &coord_key).is_none(),
        "claim row exists but the kind:30023 event has not arrived yet"
    );

    // Deliver the matching signed kind:30023 EVENT through production ingest.
    kernel.handle_text(
        RelayRole::Indexer,
        relay_url,
        &event_frame(shared_sub_id, &event),
    );

    // The addressable article MUST now surface in refs.event, keyed by
    // the coordinate string.
    let entry = event_ref_row(&mut kernel, &mut event_store, &coord_key).expect(
        "claimed kind:30023 article must surface in refs.event after arriving \
         on the production wire path",
    );
    assert_eq!(entry.primary_id, coord_key);
    assert_eq!(entry.kind, 30023);
    assert_eq!(entry.author_pubkey, author_hex);

    test_support::clear_claim_expansion_subs();
}
