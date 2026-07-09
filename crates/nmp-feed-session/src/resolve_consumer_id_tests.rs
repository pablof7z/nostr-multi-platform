//! `unique_consumer_id` — the shared per-instance-uniqueness mechanism (split
//! out of `resolve_tests.rs` for file-size discipline — pre-#3086-merge
//! polish).
//!
//! #3085 fixed the follow-set resolver's hardcoded `consumer_id`; #3086 SHOULD-
//! FIX 4 routed `resolve_wot`/`pointer_target_hydration` through the same
//! helper. A follow-up sweep found THREE more sibling resolvers
//! (`nip51_sources::resolve_list_members`'s people-list AND active-mute-list
//! paths, `nip29_group_sources::resolve_active_simple_groups`) still passing a
//! hardcoded literal. Every one of these six call sites shares this single
//! mechanism, so proving the mechanism itself mints a distinct id per call is
//! the load-bearing regression guard for the whole class — a composite/
//! multi-lane feed that resolves the SAME scope from independent lanes (e.g.
//! two `ListMembers` lanes over the same list, or two hosted-group lanes) must
//! get two independent refcount-owner keys, never one shared literal that lets
//! the first lane to close tear down the subscription still used by the other.

use super::resolve::unique_consumer_id;

#[test]
fn unique_consumer_id_mints_a_distinct_id_per_call_with_the_same_prefix() {
    let a = unique_consumer_id("nmp.feed.resolver.people_list");
    let b = unique_consumer_id("nmp.feed.resolver.people_list");
    assert_ne!(
        a, b,
        "two resolver instances over the SAME scope must get distinct owner keys"
    );
    assert!(a.starts_with("nmp.feed.resolver.people_list#"));
    assert!(b.starts_with("nmp.feed.resolver.people_list#"));
}

#[test]
fn unique_consumer_id_is_distinct_across_all_six_swept_call_site_prefixes() {
    // A cheap sanity sweep: none of the six resolver call sites (#3085's
    // follow_set + #3086's wot/pointer_target_hydration + this sweep's
    // people_list/active_mute_list/simple_groups) can collide with each other
    // even if two different scopes happened to mint at "the same moment" —
    // the counter is process-wide, so every mint is globally distinct
    // regardless of prefix.
    let ids: Vec<String> = [
        "nmp.feed.resolver.follow_set",
        "nmp.feed.resolver.wot",
        "nmp.feed.resolver.pointer_target_hydration.pointer",
        "nmp.feed.resolver.people_list",
        "nmp.feed.resolver.active_mute_list",
        "nmp.feed.resolver.simple_groups",
    ]
    .into_iter()
    .map(unique_consumer_id)
    .collect();
    let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "every minted id must be distinct");
}
