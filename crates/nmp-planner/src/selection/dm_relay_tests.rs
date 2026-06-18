//! Regression: NIP-17 DM-inbox relays must bypass greedy coverage pruning
//! (#1493 P7).
//!
//! A gift-wrap DM inbox interest (Case C: my own pubkey in `#p`, no author
//! filter) compiles to an empty-author *wildcard* sub-shape carrying
//! `RoutingSource::Nip17DmRelay`. Wildcard sub-shapes score zero coverage in
//! the greedy max-coverage pass, so before this fix the DM inbox relay survived
//! only through the budget-bounded wildcard backfill loop — which stops once
//! `max_connections` is exhausted by NIP-65 outbox relays. Under a large follow
//! set the DM inbox relay was silently pruned and the user stopped receiving
//! direct messages.
//!
//! kind:10050 DM relays are a protocol-mandated inbox, not an optimizable
//! kind:10002 outbox; they must bypass selection exactly like
//! Hint/Provenance/AppRelay.

use super::*;
use crate::{
    interest::InterestShape,
    plan::{canonical_filter_hash, RelayAttribution, RelayPlan, SubShape},
};

/// Build a plan where each relay carries one sub-shape with the supplied author
/// set (empty = a `#p`-scoped wildcard, the Case C gift-wrap inbox shape) and
/// the supplied routing sources. A `#p` tag is attached when the author set is
/// empty so the shape mirrors a real DM-inbox sub-shape rather than a degenerate
/// match-everything filter.
fn plan_with_sources(relays: &[(&str, &[&str], &[RoutingSource])]) -> CompiledPlan {
    let mut per_relay = BTreeMap::new();
    for (relay, authors, sources) in relays {
        let mut shape = InterestShape::default();
        for a in *authors {
            shape.authors.insert((*a).to_string());
        }
        if authors.is_empty() {
            // Case C gift-wrap inbox: `#p = {me}`, no author filter.
            shape
                .tags
                .insert("p".to_string(), std::iter::once("me".to_string()).collect());
        }
        let hash = canonical_filter_hash(&shape);
        let sub = SubShape {
            shape,
            originating_interests: vec![],
            canonical_filter_hash: hash,
        };
        let mut role_tags = BTreeSet::new();
        for src in *sources {
            role_tags.insert(src.clone());
        }
        per_relay.insert(
            (*relay).to_string(),
            RelayPlan {
                relay_url: (*relay).to_string(),
                role_tags,
                sub_shapes: vec![sub],
                attribution: RelayAttribution::default(),
            },
        );
    }
    CompiledPlan {
        plan_id: "test".to_string(),
        per_relay,
        unroutable_authors: BTreeSet::new(),
    }
}

/// The core #1493-P7 regression: a DM inbox relay (empty-author wildcard,
/// `Nip17DmRelay`-tagged) must survive even when NIP-65 outbox relays consume
/// the entire `max_connections` budget.
#[test]
fn dm_inbox_relay_survives_when_outbox_relays_exhaust_the_connection_budget() {
    let nip65 = [RoutingSource::Nip65];
    let dm = [RoutingSource::Nip17DmRelay];
    // Two NIP-65 outbox relays, each covering a distinct author, plus the DM
    // inbox relay. With max_connections=2 the greedy pass picks both author
    // relays and the budget is fully spent; the wildcard backfill would never
    // fire. Only the bypass keeps the DM relay alive.
    let mut plan = plan_with_sources(&[
        ("wss://atlas.nostr.land", &["gigi"], &nip65),
        ("wss://eden.nostr.land", &["fiatjaf"], &nip65),
        ("wss://dm.inbox.example", &[], &dm),
    ]);
    apply_selection(&mut plan, 2, 2);

    assert!(
        plan.per_relay.contains_key("wss://dm.inbox.example"),
        "NIP-17 DM-inbox relay must survive selection even when the \
         connection budget is exhausted by NIP-65 outbox relays; got: {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );
    // The wildcard sub-shape is preserved unchanged (it had no authors to
    // filter), so its `#p` scope and canonical hash stay intact.
    let dm_relay = &plan.per_relay["wss://dm.inbox.example"];
    assert_eq!(dm_relay.sub_shapes.len(), 1);
    assert!(dm_relay.sub_shapes[0].shape.authors.is_empty());
    assert_eq!(
        dm_relay.sub_shapes[0].shape.tags.get("p"),
        Some(&std::iter::once("me".to_string()).collect()),
        "DM inbox sub-shape must keep its `#p` scope through selection",
    );
}

/// Tightest possible budget: NIP-65 relays alone exceed `max_connections`, so
/// not even the wildcard backfill could run. The DM relay must still survive.
#[test]
fn dm_inbox_relay_survives_under_max_connections_one() {
    let nip65 = [RoutingSource::Nip65];
    let dm = [RoutingSource::Nip17DmRelay];
    let mut plan = plan_with_sources(&[
        ("wss://atlas.nostr.land", &["gigi"], &nip65),
        ("wss://eden.nostr.land", &["fiatjaf"], &nip65),
        ("wss://dm.inbox.example", &[], &dm),
    ]);
    apply_selection(&mut plan, 1, 2);

    assert!(
        plan.per_relay.contains_key("wss://dm.inbox.example"),
        "DM inbox relay must survive even when max_connections=1; got: {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );
}

/// A relay that carries BOTH a NIP-65 author sub-shape AND the `Nip17DmRelay`
/// lane survives unchanged (the dual-lane carve-out), without its NIP-65 author
/// being pruned.
#[test]
fn dual_lane_dm_plus_nip65_relay_survives() {
    let nip65 = [RoutingSource::Nip65];
    let dual = [RoutingSource::Nip65, RoutingSource::Nip17DmRelay];
    let mut plan = plan_with_sources(&[
        ("wss://atlas.nostr.land", &["gigi"], &nip65),
        ("wss://eden.nostr.land", &["fiatjaf"], &nip65),
        ("wss://dm.inbox.example", &["gigi"], &dual),
    ]);
    apply_selection(&mut plan, 1, 2);

    assert!(
        plan.per_relay.contains_key("wss://dm.inbox.example"),
        "dual-lane DM relay must survive; got: {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );
    // Pinned relays are projected unchanged, so the author stays on the
    // sub-shape (selection must not narrow a bypassed relay's author set).
    let dm_relay = &plan.per_relay["wss://dm.inbox.example"];
    assert!(
        dm_relay.sub_shapes[0].shape.authors.contains("gigi"),
        "bypassed DM relay must keep its author unchanged; got {:?}",
        dm_relay.sub_shapes[0].shape.authors,
    );
}
