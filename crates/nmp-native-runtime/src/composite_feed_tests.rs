//! `NmpApp::open_composite_feed` reachability proofs (#3086 BLOCKER 1).
//!
//! The deep, real-relay driving-example + target-first-ordering proofs live in
//! `crates/nmp-testing/tests/composite_feed_driving_example.rs` (a real
//! `NmpApp` + real relay, through `open_composite_feed_with_mappings_for_test`).
//! These narrower, in-crate tests prove the composition-root wiring itself:
//! the production lane-mapping registry carries all three registered ids, and
//! `NmpApp::open_composite_feed` — the canonical entry point, previously
//! UNREACHABLE from any caller — actually opens and tears down a session over
//! the existing session-registry mechanics.

use std::collections::{BTreeMap, BTreeSet};

use nmp_feed::{
    CompositeFeedParams, FeedItemProjection, FeedLane, FeedScope, FeedWindowPolicy, LaneMappingId,
    ProjectionKey, SortPolicy, DIRECT_MAPPING_ID,
};

use crate::composite_feed::composite_lane_mappings;
use crate::new_app;

#[test]
fn composite_lane_mappings_registers_all_three_production_ids() {
    let registry = composite_lane_mappings();
    assert!(
        registry
            .get(&LaneMappingId(DIRECT_MAPPING_ID.to_string()))
            .is_some(),
        "feed.authored must be pre-installed"
    );
    assert!(
        registry
            .get(&LaneMappingId(nmp_nip18::NIP18_TARGET_MAPPING_ID.to_string()))
            .is_some(),
        "nip18.target must be registered at the composition root"
    );
    assert!(
        registry
            .get(&LaneMappingId(nmp_nip22::NIP22_ROOT_MAPPING_ID.to_string()))
            .is_some(),
        "nip22.root must be registered at the composition root"
    );
}

/// #3086 BLOCKER 1 — `open_composite_feed` had zero callers before this fix.
/// A single-lane composite (the degenerate case) over a static author set
/// proves the canonical entry point is wired end to end: registry lookup →
/// `custom::resolve_acquisition` → `build_flat_scope_session` → a live handle
/// that tears down cleanly through the SAME session registry `open_feed` uses.
#[test]
fn open_composite_feed_opens_and_closes_a_single_lane_session() {
    let app = new_app();

    let params = CompositeFeedParams {
        key: ProjectionKey::app_owned("test.composite.reachability").unwrap(),
        lanes: vec![FeedLane {
            source: FeedScope::Authors {
                authors: BTreeSet::from(["a".repeat(64)]),
            },
            match_kinds: vec![1],
            match_tags: BTreeMap::new(),
            mapping: LaneMappingId(DIRECT_MAPPING_ID.to_string()),
        }],
        render_target_kinds: Vec::new(),
        sort: SortPolicy::ByInteractionTime,
        window: FeedWindowPolicy::bounded(80),
        item_projection: FeedItemProjection::FeedRows,
    };

    let handle = app
        .open_composite_feed(&params)
        .expect("a single-lane composite feed opens through the real composition root");
    assert!(app.feed_session_is_open(&handle));
    assert_eq!(app.live_feed_session_count(), 1);

    assert!(app.close_feed(&handle));
    assert_eq!(app.live_feed_session_count(), 0);
}

/// An unregistered mapping id fails closed (D6) with no partial registration
/// — proving the registry-lookup fail path composite_compiler.rs's
/// `revoke_all` exists for is actually reachable from the real entry point.
#[test]
fn open_composite_feed_fails_closed_on_unregistered_mapping() {
    let app = new_app();

    let params = CompositeFeedParams {
        key: ProjectionKey::app_owned("test.composite.unregistered-mapping").unwrap(),
        lanes: vec![FeedLane {
            source: FeedScope::Authors {
                authors: BTreeSet::from(["a".repeat(64)]),
            },
            match_kinds: vec![1],
            match_tags: BTreeMap::new(),
            mapping: LaneMappingId("nothing.registered.under.this.id".to_string()),
        }],
        render_target_kinds: Vec::new(),
        sort: SortPolicy::ByInteractionTime,
        window: FeedWindowPolicy::bounded(80),
        item_projection: FeedItemProjection::FeedRows,
    };

    assert!(app.open_composite_feed(&params).is_err());
    assert_eq!(app.live_feed_session_count(), 0);
}
