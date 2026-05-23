use super::super::*;
use crate::kernel::types::AuthorRelayList;
use crate::relay::{BOOTSTRAP_DISCOVERY_RELAYS, DEFAULT_VISIBLE_LIMIT};

fn relay_list(read: &[&str], write: &[&str], both: &[&str]) -> AuthorRelayList {
    AuthorRelayList {
        event_id: "x".to_string(),
        created_at: 1,
        read_relays: read.iter().map(|s| s.to_string()).collect(),
        write_relays: write.iter().map(|s| s.to_string()).collect(),
        both_relays: both.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn author_write_relays_returns_write_plus_both_when_cached() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.author_relay_lists.insert(
        "alice".to_string(),
        relay_list(&["wss://r.in"], &["wss://r.out"], &["wss://r.both"]),
    );

    let relays = kernel.author_write_relays("alice");
    assert_eq!(relays, vec!["wss://r.both", "wss://r.out"]);
}

#[test]
fn author_write_relays_falls_back_to_bootstrap_when_uncached() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let relays = kernel.author_write_relays("never-seen");
    assert_eq!(
        relays,
        kernel.bootstrap_discovery_relays()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn author_write_relays_falls_back_when_all_buckets_empty() {
    // Defensive: an entry with no write/both falls back to bootstrap so
    // we don't silently drop the author from the plan.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel
        .author_relay_lists
        .insert("alice".to_string(), relay_list(&["wss://r.in"], &[], &[]));
    let relays = kernel.author_write_relays("alice");
    assert_eq!(
        relays,
        kernel.bootstrap_discovery_relays()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn partition_authors_groups_by_resolved_write_relays() {
    // Two authors with DISTINCT write relays — the test the task pins:
    // a follow-feed REQ must fan out to each followed author's resolved
    // write relays, NOT the constants.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.author_relay_lists.insert(
        "alice".to_string(),
        relay_list(&[], &["wss://alice.relay"], &[]),
    );
    kernel.author_relay_lists.insert(
        "bob".to_string(),
        relay_list(&[], &["wss://bob.relay"], &["wss://shared.relay"]),
    );
    let parts = kernel
        .partition_authors_by_write_relays(&["alice".to_string(), "bob".to_string()]);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts.get("wss://alice.relay").unwrap(), &vec!["alice"]);
    assert_eq!(parts.get("wss://bob.relay").unwrap(), &vec!["bob"]);
    assert_eq!(parts.get("wss://shared.relay").unwrap(), &vec!["bob"]);
}

#[test]
fn partition_authors_uses_bootstrap_for_uncached_authors() {
    // Cold-start: author has no cached kind:10002. The bootstrap seed
    // must appear in the plan so the first discovery REQ has somewhere
    // to leave on; once the kind:10002 arrives the next emission
    // re-partitions onto the resolved relays (A1 recompilation trigger).
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parts = kernel.partition_authors_by_write_relays(&["uncached".to_string()]);
    for seed in BOOTSTRAP_DISCOVERY_RELAYS {
        assert!(
            parts.contains_key(*seed),
            "bootstrap seed {seed} must serve uncached author"
        );
    }
}

#[test]
fn all_authors_have_relay_lists_distinguishes_cold_warm() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(!kernel.all_authors_have_relay_lists(&["alice".to_string()]));
    kernel
        .author_relay_lists
        .insert("alice".to_string(), relay_list(&[], &["wss://a"], &[]));
    assert!(kernel.all_authors_have_relay_lists(&["alice".to_string()]));
    assert!(!kernel
        .all_authors_have_relay_lists(&["alice".to_string(), "bob".to_string()]));
}

#[test]
fn recipient_read_relays_returns_read_plus_both() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.author_relay_lists.insert(
        "bob".to_string(),
        relay_list(&["wss://r.in"], &["wss://r.out"], &["wss://r.both"]),
    );
    let relays = kernel.recipient_read_relays("bob");
    assert_eq!(relays, vec!["wss://r.both", "wss://r.in"]);
}

// ── T132 parity tests ────────────────────────────────────────────────
//
// After T132, the planner consumes mailbox data through a `KernelMailboxes`
// adapter that borrows `Kernel::author_relay_lists`. These tests pin the
// invariant the task closes: the publish-path resolver
// (`author_write_relays`) and the planner-path adapter return identical
// data for the same NIP-65 input. If they ever drift, the kernel-managed
// ingest path and the planner compile path will be looking at different
// truths — exactly the dual-source-of-truth hazard T132 was filed to fix.

#[test]
fn t132_parity_publish_path_and_planner_adapter_agree_on_kind10002() {
    use crate::planner::MailboxCache;
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.author_relay_lists.insert(
        "alice".to_string(),
        relay_list(
            &["wss://r.read"],
            &["wss://r.write.a", "wss://r.write.b"],
            &["wss://r.both"],
        ),
    );

    // Publish-path view: write + both, sorted/deduped.
    let publish_path = kernel.author_write_relays("alice");
    assert_eq!(
        publish_path,
        vec!["wss://r.both", "wss://r.write.a", "wss://r.write.b"]
    );

    // Planner-path view via the adapter — outbox_relays iterates
    // write ∪ both in the same order they appear in the snapshot.
    let view = kernel.mailbox_cache_view();
    let snap = view.get(&"alice".to_string()).expect("alice cached");
    let mut planner_path: Vec<String> = snap.outbox_relays().cloned().collect();
    sort_dedup(&mut planner_path);
    assert_eq!(planner_path, publish_path);
}

#[test]
fn t132_parity_empty_kind10002_clears_both_views() {
    use crate::planner::MailboxCache;
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel
        .author_relay_lists
        .insert("alice".to_string(), relay_list(&[], &["wss://a"], &[]));
    // Simulate the "empty kind:10002" branch of ingest_relay_list — the
    // entry is removed entirely (see relay_list.rs lines 30-36).
    kernel.author_relay_lists.remove("alice");

    // Publish path falls back to bootstrap seed.
    let publish_path = kernel.author_write_relays("alice");
    assert_eq!(
        publish_path,
        kernel.bootstrap_discovery_relays()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );

    // Planner adapter sees None (cold-start) — the planner Case A then
    // routes the author through indexer_relays / bootstrap, matching the
    // publish-path fallback semantically (both surfaces use the same
    // cold-start fallback strategy via their respective code paths).
    let view = kernel.mailbox_cache_view();
    assert!(view.get(&"alice".to_string()).is_none());
}

#[test]
fn t132_parity_newer_kind10002_supersedes_on_both_views() {
    use crate::planner::MailboxCache;
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Older entry.
    kernel.author_relay_lists.insert(
        "alice".to_string(),
        AuthorRelayList {
            event_id: "older".to_string(),
            created_at: 100,
            read_relays: vec![],
            write_relays: vec!["wss://old.write".to_string()],
            both_relays: vec![],
        },
    );
    // Newer entry replaces (simulating the should_replace branch in
    // ingest_relay_list).
    kernel.author_relay_lists.insert(
        "alice".to_string(),
        AuthorRelayList {
            event_id: "newer".to_string(),
            created_at: 200,
            read_relays: vec![],
            write_relays: vec!["wss://new.write".to_string()],
            both_relays: vec![],
        },
    );

    // Publish path returns only the new write relay.
    let publish_path = kernel.author_write_relays("alice");
    assert_eq!(publish_path, vec!["wss://new.write".to_string()]);

    // Planner adapter sees the same new data.
    let view = kernel.mailbox_cache_view();
    let snap = view.get(&"alice".to_string()).expect("alice cached");
    let planner_path: Vec<String> = snap.outbox_relays().cloned().collect();
    assert_eq!(planner_path, vec!["wss://new.write".to_string()]);
}

// ── role_for_relay_url canonicalization (T105 / T-relay-url-normalize) ──
//
// `RelayEditRow.url` is always stored canonical (`add_relay` canonicalizes
// before insert). `role_for_relay_url` must canonicalize its *input* too,
// or a raw / mixed-case caller URL silently misses the matching edit row
// and mislabels the transport lane as Content.

#[test]
fn role_for_relay_url_matches_indexer_via_noncanonical_input() {
    use crate::relay::RelayRole;
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Edit row stored canonical (lowercase host, no trailing slash) — the
    // exact form `add_relay` would persist.
    kernel.set_relay_edit_rows(vec![RelayEditRow::new(
        "wss://purplepag.es".to_string(),
        "indexer".to_string(),
    )]);

    // A non-canonical caller input (mixed-case host + trailing slash) must
    // still resolve to the Indexer lane, not fall through to Content.
    assert_eq!(
        kernel.role_for_relay_url("wss://Purplepag.es/"),
        Some(RelayRole::Indexer),
        "non-canonical input must canonicalize before matching edit row"
    );
    // Canonical input keeps working.
    assert_eq!(
        kernel.role_for_relay_url("wss://purplepag.es"),
        Some(RelayRole::Indexer),
    );
}

#[test]
fn role_for_relay_url_unknown_url_falls_back_to_content() {
    use crate::relay::RelayRole;
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No edit rows configured — any URL falls through to the Content lane.
    assert_eq!(
        kernel.role_for_relay_url("wss://some.unknown.relay"),
        Some(RelayRole::Content),
    );
}

#[test]
fn t132_recompile_uses_kernel_mailbox_cache_for_plan_partition() {
    // The seam-proof test: build a SubscriptionLifecycle, push a
    // LogicalInterest with `alice` as the author, and feed it the kernel's
    // mailbox view. Assert the resulting plan partitions onto alice's
    // resolved write relays, NOT the indexer / bootstrap seed.
    use crate::planner::{
        InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
    };
    use crate::subs::SubscriptionLifecycle;
    use std::collections::BTreeSet;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.author_relay_lists.insert(
        "alice-pubkey".to_string(),
        relay_list(&[], &["wss://alice.write"], &[]),
    );

    let mut lifecycle = SubscriptionLifecycle::new();
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: {
                let mut s = BTreeSet::new();
                s.insert("alice-pubkey".to_string());
                s
            },
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    };
    lifecycle.registry_mut().push(interest);

    let view = kernel.mailbox_cache_view();
    let frames = lifecycle
        .recompile_and_diff(&view)
        .expect("recompile should succeed");

    // The plan must include at least one REQ on alice's resolved write
    // relay — proving the kernel-side mailbox view fed the planner, not
    // the (now-deleted) lifecycle-internal cache.
    let alice_relay_frames: Vec<_> = frames
        .iter()
        .filter(|f| match f {
            crate::subs::WireFrame::Req { relay_url, .. } => {
                relay_url == "wss://alice.write"
            }
            _ => false,
        })
        .collect();
    assert!(
        !alice_relay_frames.is_empty(),
        "expected at least one REQ on alice's resolved write relay; got: {frames:?}",
    );
}
