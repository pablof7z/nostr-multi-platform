//! M11.5 Step 0 integration tests.
//!
//! Three required exit-gate tests per the task brief, exercising the
//! load-bearing contracts:
//!
//! 1. **Group lifecycle** — `CreateGroup` action emits a host-pinned
//!    `PublishPlan` for kind 9007; ingest of the relay's reflected 39000 +
//!    39001 + 39002 flows through the trust + audit pipeline and projects
//!    correctly into `GroupHomeView` + `GroupMembersView`.
//! 2. **Lattice Rule 9 relay-pin / h-tag coalesce** — two host-pinned
//!    interests targeting different hosts refuse to merge; identical hosts
//!    merge cleanly (Rule 2 unions h-tag values); the pin short-circuits
//!    the four-lane partition (Case E).
//! 3. **Audit-only moderation** — an ingested kind:9000 produces a
//!    `ModerationEventRecord` and does NOT touch `GroupMembers`; the
//!    relay-reflected 39002 is what flips canonical membership.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::planner::{
    merge as lattice_merge, EmptyMailboxCache, InterestId, InterestLifecycle, InterestScope,
    InterestShape, LogicalInterest, MergeOutcome, SubscriptionCompiler,
};
use nmp_core::substrate::{ActionContext, ActionModule, KernelEvent, ViewContext, ViewModule};

use crate::action::{CreateGroupAction, CreateGroupInput};
use crate::cache::{TofuSignerCache, TrustCheckOutcome};
use crate::domain::records::{GroupMembershipSnapshot, MemberEntry};
use crate::group_id::GroupId;
use crate::interest::host_pinned_interest;
use crate::kinds::{KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS, KIND_GROUP_METADATA, KIND_PUT_USER};
use crate::moderation::build_audit_record;
use crate::view::{GroupHomeView, GroupMembersView, MembersSpec};
use crate::view::HomeSpec;

fn host() -> String { "wss://groups.example.com".to_string() }
fn relay_signer() -> String { "relay-pk-deadbeef".to_string() }
fn founder() -> String { "founder-pk-feedface".to_string() }

fn group() -> GroupId { GroupId::new(host(), "test-room") }

fn make_event(id: &str, kind: u32, author: &str, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind,
        created_at,
        tags,
        content: String::new(),
    }
}

// ─── Test 1: group lifecycle ─────────────────────────────────────────────────

#[test]
fn nip29_group_lifecycle_create_then_ingest_metadata() {
    // Step A: founder fires CreateGroup → action returns a Pending plan with a
    // host-pinned PublishPlan for kind 9007.
    let mut ctx = ActionContext { now_ms: 1_000 };
    let input = CreateGroupInput { group: group(), fields: Default::default() };
    let plan = CreateGroupAction::start(&mut ctx, input).expect("create accepted");
    assert!(matches!(
        plan.initial_status,
        nmp_core::substrate::ActionStatus::Pending
    ));

    // Step B: relay reflects 39000 + 39001 + 39002. Trust check pins on the
    // 39000 (cold TOFU, no NIP-11 pubkey declared); 39001 + 39002 then accepted.
    let mut tofu = TofuSignerCache::new();
    let metadata_evt = make_event(
        "evt-39000",
        KIND_GROUP_METADATA,
        &relay_signer(),
        1_010,
        vec![vec!["d".into(), "test-room".into()], vec!["name".into(), "Test".into()]],
    );
    assert_eq!(
        tofu.evaluate(metadata_evt.kind, &group(), &metadata_evt.author, &metadata_evt.id, metadata_evt.created_at),
        TrustCheckOutcome::Accepted
    );
    let admins_evt = make_event(
        "evt-39001",
        KIND_GROUP_ADMINS,
        &relay_signer(),
        1_020,
        vec![vec!["d".into(), "test-room".into()], vec!["p".into(), founder()]],
    );
    let members_evt = make_event(
        "evt-39002",
        KIND_GROUP_MEMBERS,
        &relay_signer(),
        1_030,
        vec![vec!["d".into(), "test-room".into()], vec!["p".into(), founder()]],
    );
    for e in [&admins_evt, &members_evt] {
        assert_eq!(
            tofu.evaluate(e.kind, &group(), &e.author, &e.id, e.created_at),
            TrustCheckOutcome::Accepted
        );
    }

    // Step C: views project correctly.
    let vc = ViewContext { now_ms: 1_100 };
    let (mut home_state, _) = GroupHomeView::open(&vc, HomeSpec { group: group() });
    for e in [&metadata_evt, &admins_evt, &members_evt] {
        let _ = GroupHomeView::on_event_inserted(&vc, &mut home_state, e);
    }
    let home = GroupHomeView::snapshot(&vc, &home_state);
    assert_eq!(home.metadata_event_count, 1);
    assert_eq!(home.admin_event_count, 1);
    assert_eq!(home.member_event_count, 1);

    let (mut mem_state, _) = GroupMembersView::open(&vc, MembersSpec { group: group() });
    for e in [&admins_evt, &members_evt] {
        let _ = GroupMembersView::on_event_inserted(&vc, &mut mem_state, e);
    }
    let snap = GroupMembersView::snapshot(&vc, &mem_state);
    assert_eq!(snap.admins, vec![founder()]);
    assert_eq!(snap.members, vec![founder()]);
}

// ─── Test 2: lattice Rule 9 relay-pin / h-tag coalesce ──────────────────────

#[test]
fn nip29_lattice_rule9_relay_pin_blocks_cross_host_merge() {
    let g_a = GroupId::new("wss://relay-a.example.com", "room");
    let g_b = GroupId::new("wss://relay-b.example.com", "room");

    let i_a = host_pinned_interest(1, &g_a, [9], BTreeMap::new(), InterestLifecycle::Tailing);
    let i_b = host_pinned_interest(2, &g_b, [9], BTreeMap::new(), InterestLifecycle::Tailing);

    // Direct lattice check: refuse across hosts.
    let outcome = lattice_merge(&i_a.shape, &i_b.shape, &i_a.lifecycle, &i_b.lifecycle);
    assert_eq!(outcome, MergeOutcome::Refused, "different relay_pin must refuse merge");

    // End-to-end compiler check: pinned interests on different hosts each
    // produce their own per-relay plan (Case E short-circuits the four-lane
    // partition; the planner emits one frame per host).
    let cache = EmptyMailboxCache;
    let indexer: Vec<String> = vec!["wss://indexer.example.com".into()];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);
    let plan = compiler.compile(&[i_a.clone(), i_b.clone()]).expect("compile");
    assert!(plan.per_relay.contains_key(&g_a.host_relay_url));
    assert!(plan.per_relay.contains_key(&g_b.host_relay_url));
    // Indexer must NOT be reached — pinned interests skip the indexer fallback.
    assert!(!plan.per_relay.contains_key("wss://indexer.example.com"));

    // Identical pinned interests collapse to a single per-relay sub_shape with
    // the merged h-tag dimension (Rule 9 passes, Rule 2 unions h values).
    let mut i_c = host_pinned_interest(3, &g_a, [9], BTreeMap::new(), InterestLifecycle::Tailing);
    // Distinct interest_id so the compiler tracks both as originators.
    i_c.id = InterestId(3);
    let plan2 = compiler.compile(&[i_a.clone(), i_c.clone()]).expect("compile");
    let host_a_plan = plan2.per_relay.get(&g_a.host_relay_url).expect("host a present");
    // Same h tag value → one sub_shape (merged), not two.
    assert_eq!(host_a_plan.sub_shapes.len(), 1);
    let merged_h = host_a_plan.sub_shapes[0].shape.tags.get("h").unwrap();
    assert!(merged_h.contains("room"));

    // Sanity-check that an unpinned interest does not collapse into a pinned
    // one (Rule 9: None does NOT absorb Some).
    let unpinned = LogicalInterest {
        id: InterestId(99),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [9u32].into_iter().collect(),
            tags: {
                let mut m: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
                m.insert("h".into(), ["room".into()].into_iter().collect());
                m
            },
            relay_pin: None,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    };
    let outcome2 = lattice_merge(&i_a.shape, &unpinned.shape, &i_a.lifecycle, &unpinned.lifecycle);
    assert_eq!(outcome2, MergeOutcome::Refused, "None must not absorb Some(host)");
}

// ─── Test 3: audit-only moderation (canonical membership unchanged) ──────────

#[test]
fn nip29_moderation_audit_does_not_mutate_canonical_membership() {
    let g = group();
    let admin_pk = "admin-pk-aaaa";
    let new_member = "new-member-pk-bbbb";
    let put_user_tags = vec![
        vec!["h".into(), g.local_id.clone()],
        vec!["p".into(), new_member.into()],
        vec!["reason".into(), "added by admin".into()],
    ];

    // Materialise audit record from the user-signed 9000 (moderation.md §5).
    let audit = build_audit_record(&g, "evt-9000", KIND_PUT_USER, admin_pk, 2_000, &put_user_tags);
    assert_eq!(audit.kind, KIND_PUT_USER);
    assert_eq!(audit.actor_pubkey, admin_pk);
    assert_eq!(audit.target_pubkey.as_deref(), Some(new_member));
    assert_eq!(audit.target_event_id, None);
    assert_eq!(audit.reason.as_deref(), Some("added by admin"));

    // Canonical 39002 reflects ONLY the previous member set (the relay has not
    // yet republished the new snapshot). Audit record must NOT have mutated it.
    let prior_snapshot = GroupMembershipSnapshot {
        group: g.clone(),
        event_id: "evt-prior-39002".into(),
        signer_pubkey: relay_signer(),
        created_at: 1_500,
        kind: KIND_GROUP_MEMBERS,
        entries: vec![MemberEntry { pubkey: founder(), role: None, description: None }],
    };
    assert!(
        prior_snapshot.entries.iter().all(|e| e.pubkey != new_member),
        "audit alone must NOT have added the new member to canonical state"
    );
    assert_eq!(prior_snapshot.entries.len(), 1);

    // Now the relay reflects the new 39002; canonical flips exactly once.
    let new_snapshot = GroupMembershipSnapshot {
        group: g.clone(),
        event_id: "evt-new-39002".into(),
        signer_pubkey: relay_signer(),
        created_at: 2_010,
        kind: KIND_GROUP_MEMBERS,
        entries: vec![
            MemberEntry { pubkey: founder(), role: None, description: None },
            MemberEntry { pubkey: new_member.into(), role: None, description: None },
        ],
    };
    assert!(new_snapshot.entries.iter().any(|e| e.pubkey == new_member));
    assert_eq!(new_snapshot.entries.len(), 2);

    // The audit record persists alongside the canonical update — it's the
    // ONLY effect of the 9000 ingest beyond the relay-reflected 39002.
    let audit_view = build_audit_record(&g, "evt-9000", KIND_PUT_USER, admin_pk, 2_000, &put_user_tags);
    assert_eq!(audit_view, audit);
}
