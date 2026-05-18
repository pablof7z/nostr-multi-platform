//! T82 integration tests — the discovery seam end-to-end through the kernel.
//!
//! Exercises `collect_unknown_refs` (ingest seam) → `drain_unknown_oneshots`
//! (registry + wire) → `complete_unknown_oneshot` (EOSE release), including
//! the load-bearing acceptance criterion: a quoted-note's missing id is
//! discovered and resolvable via a oneshot.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const QUOTED_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const MENTIONED_PK: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const KNOWN_ID: &str = "3333333333333333333333333333333333333333333333333333333333333333";

fn tag(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[test]
fn quoted_note_missing_id_is_discovered_and_resolvable_via_oneshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // A kind:1 note quoting an event we do not have, plus a p-tag mention of
    // an unknown pubkey. This is the ingest seam input (borrowed visitor).
    let tags = vec![
        tag(&["q", QUOTED_ID]),
        tag(&["p", MENTIONED_PK]),
    ];
    kernel.collect_unknown_refs(&tags);

    // Drain → oneshot interests registered + M1 REQ frames emitted.
    let reqs = kernel.drain_unknown_oneshots();
    assert_eq!(reqs.len(), 2, "one oneshot per missing reference");
    assert_eq!(kernel.discovery_in_flight(), 2);

    let joined = reqs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains(QUOTED_ID),
        "quoted-note id is fetched by id"
    );
    assert!(
        joined.contains(MENTIONED_PK) && joined.contains("\"kinds\":[0]"),
        "mentioned pubkey is fetched as a kind:0 profile oneshot"
    );
    // Discovery oneshots use the reserved sub-id prefix.
    assert!(reqs
        .iter()
        .all(|r| r.text.contains(discovery::ONESHOT_SUB_PREFIX)));

    // Resolve: EOSE on each oneshot sub completes + releases its token.
    for r in &reqs {
        let sub_id = sub_id_of(&r.text);
        kernel.complete_unknown_oneshot(&sub_id);
    }
    assert_eq!(
        kernel.discovery_in_flight(),
        0,
        "all oneshots released after EOSE — no lingering subscription"
    );
}

#[test]
fn known_references_do_not_spawn_oneshots_d8_fast_path() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Seed the in-memory projection so the reference is "known".
    kernel.events.insert(
        KNOWN_ID.to_string(),
        StoredEvent {
            id: KNOWN_ID.to_string(),
            author: "a".repeat(64),
            kind: 1,
            created_at: 0,
            tags: Vec::new(),
            content: String::new(),
            relay_count: 1,
        },
    );
    kernel.collect_unknown_refs(&[tag(&["e", KNOWN_ID])]);
    let reqs = kernel.drain_unknown_oneshots();
    assert!(reqs.is_empty(), "known id is not re-fetched");
    assert_eq!(kernel.discovery_in_flight(), 0);
}

#[test]
fn drain_is_idempotent_at_kernel_level() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    assert_eq!(kernel.drain_unknown_oneshots().len(), 1);
    assert!(
        kernel.drain_unknown_oneshots().is_empty(),
        "second drain with no new refs emits nothing"
    );
}

#[test]
fn duplicate_references_across_events_dedup_before_fetch() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Same quoted id referenced by two separate ingested events.
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    kernel.collect_unknown_refs(&[tag(&["e", QUOTED_ID])]);
    let reqs = kernel.drain_unknown_oneshots();
    assert_eq!(reqs.len(), 1, "deduped to a single oneshot fetch");
}

#[test]
fn discovered_event_on_oneshot_sub_passes_the_store_gate() {
    // Regression: without the discovery prefix in `should_store_event`, a
    // resolved quoted-note arriving on its `oneshot-disc-*` sub would be
    // dropped (author isn't a timeline author), the cache would stay missing,
    // and the next ingest would re-discover + re-fetch the same id forever.
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let quoted = NostrEvent {
        id: QUOTED_ID.to_string(),
        pubkey: "f".repeat(64), // NOT a timeline author
        created_at: 1,
        kind: 1,
        tags: Vec::new(),
        content: "the quoted note".to_string(),
        sig: String::new(),
    };
    let oneshot_sub = format!("{}7", discovery::ONESHOT_SUB_PREFIX);
    assert!(
        kernel.should_store_event(&oneshot_sub, &quoted),
        "a discovered event on its oneshot sub must be storable"
    );
    // Sanity: the same event on an unrelated sub is still gated out.
    assert!(!kernel.should_store_event("some-other-sub", &quoted));
}

#[test]
fn ingest_then_drain_resolves_through_pending_view_requests() {
    // End-to-end through the kernel's own request pump: collect during ingest,
    // then `pending_view_requests` drains the unknown set into oneshot REQs.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    let pumped = kernel.pending_view_requests();
    assert!(
        pumped
            .iter()
            .any(|r| r.text.contains(QUOTED_ID)
                && r.text.contains(discovery::ONESHOT_SUB_PREFIX)),
        "pending_view_requests drains UnknownIds into a oneshot fetch"
    );
    assert_eq!(kernel.discovery_in_flight(), 1);
}

#[test]
fn completing_unknown_oneshot_for_non_discovery_sub_is_noop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Must not panic / must not touch in-flight state (D6).
    kernel.complete_unknown_oneshot("seed-timeline");
    assert_eq!(kernel.discovery_in_flight(), 0);
}

#[test]
fn many_unknown_ids_collapse_to_few_batch_reqs() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // 120 event ids -> ceil(120\/50) = 3 content REQs
    // 75 pubkeys    -> ceil(75\/50)  = 2 indexer REQs
    let tags: Vec<Vec<String>> = (0u32..120)
        .map(|i| tag(&["e", &format!("{i:0>64x}")]))
        .chain((0u32..75).map(|i| tag(&["p", &format!("{i:0>64x}")])))
        .collect();
    kernel.collect_unknown_refs(&tags);
    let reqs = kernel.drain_unknown_oneshots();
    assert_eq!(reqs.len(), 2, "throttled: 1 events REQ + 1 profiles REQ regardless of backlog size");
    assert_eq!(kernel.discovery_in_flight(), 2, "2 in-flight; 95 remain queued");
}


/// Extract the sub-id (2nd JSON array element) from a `["REQ", sub_id, …]`
/// frame text — test-local parser, avoids a serde_json dep churn here.
fn sub_id_of(req_text: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(req_text).expect("valid REQ json");
    v.get(1)
        .and_then(|s| s.as_str())
        .expect("sub-id present")
        .to_string()
}
