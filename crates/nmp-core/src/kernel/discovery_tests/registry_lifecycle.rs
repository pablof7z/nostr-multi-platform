//! Discovery-oneshot registry lifecycle: registration, dedup, idempotent
//! drain, batching under the concurrency cap, the HashMap-typed
//! `OneshotKind::Discovery` routing (T104), and the no-op EOSE-completion
//! path for a non-discovery sub.

use super::support::{
    drain_and_register, install_bootstrap_relays, planner_req_filters, tag, KNOWN_ID,
    MENTIONED_PK, QUOTED_ID,
};
use crate::kernel::{Kernel, StoredEvent};
use crate::relay::DEFAULT_VISIBLE_LIMIT;

#[test]
fn quoted_note_missing_id_is_discovered_and_resolvable_via_oneshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    // A kind:1 note quoting an event we do not have, plus a p-tag mention of
    // an unknown pubkey. This is the ingest seam input (borrowed visitor).
    let tags = vec![tag(&["q", QUOTED_ID]), tag(&["p", MENTIONED_PK])];
    kernel.collect_unknown_refs(&tags);

    // Drain → two oneshot interests registered (events + profiles arms). The
    // function no longer emits M1 OutboundMessage REQs (PD-033-C Stage 1).
    let drained = kernel.drain_unknown_oneshots();
    assert!(
        drained.is_empty(),
        "PD-033-C Stage 1: drain_unknown_oneshots must emit NO M1 \
         OutboundMessage frames; got {drained:?}"
    );
    assert_eq!(
        kernel.discovery_in_flight(),
        2,
        "one oneshot per missing reference must be registered in the registry"
    );

    // Planner side: drain_lifecycle_tick compiles the two interests into
    // WireFrame::Req frames addressed at the cold-start bootstrap relays;
    // register_planner_wire_frames bridges the planner sub_id back to the
    // OneshotToken in `oneshot_subs`.
    let frames = drain_and_register(&mut kernel);
    let filters = planner_req_filters(&frames);
    let joined_filters = filters.join("\n");
    assert!(
        joined_filters.contains(QUOTED_ID),
        "planner must emit a REQ whose filter carries the quoted-note id; \
         got filters: {filters:?}"
    );
    assert!(
        joined_filters.contains(MENTIONED_PK) && joined_filters.contains("\"kinds\""),
        "planner must emit a REQ whose filter carries the mentioned pubkey \
         under a kind-restricted (kind:0/3/10002) profile fetch; got filters: \
         {filters:?}"
    );

    // The bridge populated `oneshot_subs` keyed by the planner sub_ids so
    // every registered discovery oneshot is recognisable to the EOSE handler.
    let oneshot_sub_ids: Vec<String> = kernel.oneshot_subs.keys().cloned().collect();
    assert_eq!(
        oneshot_sub_ids.len(),
        2,
        "bridge must register both planner sub_ids in oneshot_subs"
    );
    for sub_id in &oneshot_sub_ids {
        assert!(
            kernel.is_discovery_oneshot(sub_id),
            "every bridged oneshot_subs entry must be recognised as a discovery oneshot"
        );
    }

    // Resolve: EOSE on each oneshot sub completes + releases its token.
    for sub_id in &oneshot_sub_ids {
        kernel.complete_unknown_oneshot(sub_id);
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
    let drained = kernel.drain_unknown_oneshots();
    assert!(
        drained.is_empty(),
        "known id is not re-fetched (M1 path retired anyway)"
    );
    assert_eq!(
        kernel.discovery_in_flight(),
        0,
        "known references must not register any oneshot in the registry"
    );
}

#[test]
fn drain_is_idempotent_at_kernel_level() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    // First drain registers a oneshot in the registry; second drain with no
    // new refs is a no-op (registry already at steady state).
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "first drain registers exactly one discovery oneshot"
    );
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "second drain with no new refs must not register another oneshot"
    );
}

#[test]
fn duplicate_references_across_events_dedup_before_fetch() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Same quoted id referenced by two separate ingested events.
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    kernel.collect_unknown_refs(&[tag(&["e", QUOTED_ID])]);
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "the duplicate id must dedupe into a single registered oneshot"
    );
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
    // 120 event ids -> ceil(120\/50) = 3 content REQs would be ideal, but the
    // concurrency cap (MAX_DISCOVERY_CONCURRENCY = 2) throttles us to 1 events
    // arm + 1 profiles arm per drain. The remaining 95 stay queued.
    // 75 pubkeys    -> ceil(75\/50)  = 2 indexer REQs (also throttled).
    let tags: Vec<Vec<String>> = (0u32..120)
        .map(|i| tag(&["e", &format!("{i:0>64x}")]))
        .chain((0u32..75).map(|i| tag(&["p", &format!("{i:0>64x}")])))
        .collect();
    kernel.collect_unknown_refs(&tags);
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(
        kernel.discovery_in_flight(),
        2,
        "throttled: 1 events arm + 1 profiles arm registered as oneshots; \
         95 remain queued for the next drain"
    );
}

#[test]
fn oneshot_kind_typed_routing_replaces_string_prefix_matching() {
    // T104 acceptance criterion: `is_discovery_oneshot` returns true only for
    // sub-ids registered in `oneshot_subs` with `OneshotKind::Discovery`.
    // An unregistered sub_id returns false (HashMap lookup, not prefix scan).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    // Before any drain, no sub is registered.
    let fake = "sub-deadbeef".to_string();
    assert!(
        !kernel.is_discovery_oneshot(&fake),
        "unregistered sub_id is not a discovery oneshot (HashMap lookup, not prefix scan)"
    );

    // Drain + planner tick + bridge — the canonical sub-id source is the
    // planner's `sub-<hash>` registered in `oneshot_subs` via the bridge.
    kernel.collect_unknown_refs(&[tag(&["q", QUOTED_ID])]);
    let _ = kernel.drain_unknown_oneshots();
    assert_eq!(kernel.discovery_in_flight(), 1);
    let _ = drain_and_register(&mut kernel);
    let registered_sub = kernel
        .oneshot_subs
        .keys()
        .next()
        .cloned()
        .expect("bridge must register the planner sub_id in oneshot_subs");
    assert!(
        kernel.is_discovery_oneshot(&registered_sub),
        "registered discovery oneshot is recognised by OneshotKind::Discovery lookup"
    );

    // After EOSE completes and releases the token, the sub is deregistered.
    kernel.complete_unknown_oneshot(&registered_sub);
    assert!(
        !kernel.is_discovery_oneshot(&registered_sub),
        "completed oneshot is removed from oneshot_subs — no longer recognised"
    );
}
