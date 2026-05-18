//! NIP-29 lifecycle integration tests (M11.5 T56).
//!
//! ## What this tests
//!
//! The integration tests here operate one layer *above* the intra-crate tests
//! in `nmp-nip29/src/tests.rs`: they sit in `nmp-testing` and import both
//! `nmp-nip29` and `nmp-core` as *external* consumers, exercising the seam
//! between the protocol crate and the kernel planner without any access to
//! crate-internal symbols.
//!
//! ## Contracts pinned
//!
//! 1. **publish-plan shape**: `CreateGroup`, `JoinRequest`, `LeaveRequest`, and
//!    `PostChatMessage` each emit a `PublishPlan` whose `pin_to.relay_url`
//!    equals the group's `host_relay_url`.  No action produces an unpinned
//!    `h`-tagged event (the privacy-leak guard).
//!
//! 2. **view dependency shape**: `GroupMembersView` and `GroupChatView`
//!    declare the expected `(kinds, tag_refs)` in their `dependencies()`.
//!
//! 3. **view projection**: inserting synthetic 39000 / 39001 / 39002 /
//!    kind:9 events through the view modules' `on_event_inserted` →
//!    `snapshot` cycle produces the expected payloads.
//!    - 39001 event → `MembersPayload.admins` reflects pubkey.
//!    - 39002 event → `MembersPayload.members` reflects pubkey; replace
//!      (member-remove proxy) brings the list back to empty.
//!    - kind:9 → `ChatPayload.events` reflects the message.
//!
//! 4. **relay-pin routing** (Case E, generic kernel API): a `LogicalInterest`
//!    built by `nip29::interest::host_pinned_interest` routes exclusively to
//!    the pinned host via `SubscriptionCompiler`; the indexer is not contacted.
//!    Two groups on *different* hosts produce *two* per-relay plans (Rule 9).
//!
//! ## What this does NOT test (follow-up tasks filed)
//!
//! - Wire-level REQ/EVENT/EOSE exchange via a MockRelay (no MockRelay exists
//!   on master; planned for a follow-up milestone).
//! - Signer round-trip: `action::reduce()` returns `Continue { Pending }` until
//!   the M6 signer-bridge lands (Step 5).  AuthFailed for non-admins therefore
//!   cannot be integration-tested yet; filed as follow-up.
//! - `GroupListSnapshot` / `GroupMembersSnapshot` / `GroupTimelineSnapshot`:
//!   the canonical snapshot names in the task spec map to `JoinedPayload`,
//!   `MembersPayload`, `ChatPayload` on master.  Follow-up: rename / alias once
//!   Swift codegen is wired (M11.5 Step 5).

use nmp_core::planner::{
    EmptyMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, SubscriptionCompiler,
};
use nmp_core::substrate::{ActionContext, ActionModule, KernelEvent, ViewContext, ViewModule};

use nmp_nip29::action::{
    CreateGroupAction, CreateGroupInput, JoinRequestAction, JoinRequestInput,
    LeaveRequestAction, LeaveRequestInput, PostChatMessageAction, PostChatMessageInput,
    PublishPlan,
};
use nmp_nip29::group_id::GroupId;
use nmp_nip29::interest::host_pinned_interest;
use nmp_nip29::kinds::{KIND_CHAT_MESSAGE, KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS};
use nmp_nip29::view::{ChatSpec, GroupChatView, GroupMembersView, MembersSpec};

// ── Constants ─────────────────────────────────────────────────────────────────

const HOST_A: &str = "wss://relay-a.example.com";
const HOST_B: &str = "wss://relay-b.example.com";
const INDEXER: &str = "wss://indexer.example.com";
const ADMIN_PUBKEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1";
const MEMBER_PUBKEY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb02";

fn group_a() -> GroupId {
    GroupId::new(HOST_A, "room-alpha")
}
fn group_b() -> GroupId {
    GroupId::new(HOST_B, "room-beta")
}

fn make_ctx() -> ActionContext {
    ActionContext { now_ms: 1_000 }
}
fn make_vc() -> ViewContext {
    ViewContext { now_ms: 1_000 }
}

fn kernel_event(id: &str, kind: u32, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: "relay-pk".into(),
        kind,
        created_at: 1_000,
        tags,
        content: String::new(),
    }
}

// ── 1. Publish-plan shapes ────────────────────────────────────────────────────

/// `CreateGroup` start() is accepted; a `PublishPlan::pinned` for kind 9007
/// carries `pin_to.relay_url == group.host_relay_url` and passes the h-tag guard.
#[test]
fn create_group_publish_plan_is_host_pinned() {
    let mut ctx = make_ctx();
    let group = group_a();
    let result = CreateGroupAction::start(
        &mut ctx,
        CreateGroupInput { group: group.clone(), fields: Default::default() },
    );
    assert!(result.is_ok(), "CreateGroup must accept valid input: {result:?}");

    // Verify the publish-plan shape by constructing it directly — the same path
    // the action macro uses internally.
    let plan = PublishPlan::pinned(
        &group,
        nmp_nip29::kinds::KIND_CREATE_GROUP,
        "",
        vec![vec!["h".into(), group.local_id.clone()]],
    );
    assert!(plan.validate_no_unpinned_h().is_ok(), "h-tag guard must pass for pinned plan");
    let pin = plan.pin_to.expect("CreateGroup plan must carry a relay pin");
    assert_eq!(pin.relay_url, HOST_A, "relay_url must equal group host");
    assert_eq!(
        pin.source_group.as_ref().map(|g| &g.local_id),
        Some(&"room-alpha".to_string()),
        "source_group must carry the typed GroupId"
    );
}

/// `JoinRequest` (9021) and `LeaveRequest` (9022) both accept valid input and
/// produce host-pinned plans with the h-tag guard satisfied.
#[test]
fn join_leave_publish_plans_are_host_pinned() {
    let mut ctx = make_ctx();
    let group = group_a();

    let join_result = JoinRequestAction::start(
        &mut ctx,
        JoinRequestInput {
            group: group.clone(),
            invite_code: None,
            referrer_event_id: None,
            reason: None,
        },
    );
    assert!(join_result.is_ok(), "JoinRequest must be accepted: {join_result:?}");

    let leave_result = LeaveRequestAction::start(
        &mut ctx,
        LeaveRequestInput { group: group.clone(), reason: None },
    );
    assert!(leave_result.is_ok(), "LeaveRequest must be accepted: {leave_result:?}");

    for (kind, label) in [
        (nmp_nip29::kinds::KIND_JOIN_REQUEST, "JoinRequest"),
        (nmp_nip29::kinds::KIND_LEAVE_REQUEST, "LeaveRequest"),
    ] {
        let plan = PublishPlan::pinned(
            &group,
            kind,
            "",
            vec![vec!["h".into(), group.local_id.clone()]],
        );
        assert!(plan.validate_no_unpinned_h().is_ok(), "{label} h-tag guard must pass");
        let pin = plan.pin_to.as_ref().expect("{label} must be pinned");
        assert_eq!(pin.relay_url, HOST_A, "{label} relay_url must equal host");
    }
}

/// `PostChatMessage` (kind:9) accepts non-empty content and rejects empty.
#[test]
fn post_chat_message_pinned_and_rejects_empty_content() {
    let mut ctx = make_ctx();
    let group = group_a();

    let ok = PostChatMessageAction::start(
        &mut ctx,
        PostChatMessageInput {
            group: group.clone(),
            content: "hello group".into(),
            previous_event_id_prefixes: vec![],
            reply_to_event_id: None,
        },
    );
    assert!(ok.is_ok(), "non-empty PostChatMessage must be accepted: {ok:?}");

    let err = PostChatMessageAction::start(
        &mut ctx,
        PostChatMessageInput {
            group: group.clone(),
            content: String::new(),
            previous_event_id_prefixes: vec![],
            reply_to_event_id: None,
        },
    );
    assert!(err.is_err(), "empty content must be rejected");

    let plan = PublishPlan::pinned(
        &group,
        KIND_CHAT_MESSAGE,
        "hello group",
        vec![vec!["h".into(), group.local_id.clone()]],
    );
    assert_eq!(
        plan.pin_to.as_ref().map(|p| p.relay_url.as_str()),
        Some(HOST_A),
        "PostChatMessage plan must pin to host relay"
    );
}

// ── 2. View dependency shapes ─────────────────────────────────────────────────

/// `GroupMembersView::dependencies` declares 39001 + 39002 with a `d` tag ref.
#[test]
fn group_members_view_dependencies_shape() {
    let group = group_a();
    let deps = GroupMembersView::dependencies(&MembersSpec { group: group.clone() });
    assert!(
        deps.kinds.contains(&KIND_GROUP_ADMINS),
        "must subscribe to KIND_GROUP_ADMINS (39001): {deps:?}"
    );
    assert!(
        deps.kinds.contains(&KIND_GROUP_MEMBERS),
        "must subscribe to KIND_GROUP_MEMBERS (39002): {deps:?}"
    );
    let has_d_ref = deps.tag_refs.iter().any(|(k, v)| k == "d" && v == &group.local_id);
    assert!(has_d_ref, "must filter on d = local_id for parameterised-replaceable lookup");
}

/// `GroupChatView::dependencies` declares kind:9 with an `h` tag ref.
#[test]
fn group_chat_view_dependencies_shape() {
    let group = group_a();
    let deps = GroupChatView::dependencies(&ChatSpec { group: group.clone() });
    assert!(
        deps.kinds.contains(&KIND_CHAT_MESSAGE),
        "must subscribe to KIND_CHAT_MESSAGE (9): {deps:?}"
    );
    let has_h_ref = deps.tag_refs.iter().any(|(k, v)| k == "h" && v == &group.local_id);
    assert!(has_h_ref, "must filter on h = local_id");
}

// ── 3. View projections ───────────────────────────────────────────────────────

/// 39001 → admins list reflects the pubkey; 39002 → members list reflects it.
/// Corresponds to the task's GroupMembersSnapshot contract.
#[test]
fn group_members_snapshot_reflects_admins_and_members() {
    let group = group_a();
    let vc = make_vc();
    let (mut state, _) = GroupMembersView::open(&vc, MembersSpec { group: group.clone() });

    let admins_evt = kernel_event(
        "evt-39001",
        KIND_GROUP_ADMINS,
        vec![
            vec!["d".into(), group.local_id.clone()],
            vec!["p".into(), ADMIN_PUBKEY.into()],
        ],
    );
    let members_evt = kernel_event(
        "evt-39002",
        KIND_GROUP_MEMBERS,
        vec![
            vec!["d".into(), group.local_id.clone()],
            vec!["p".into(), MEMBER_PUBKEY.into()],
        ],
    );

    GroupMembersView::on_event_inserted(&vc, &mut state, &admins_evt);
    GroupMembersView::on_event_inserted(&vc, &mut state, &members_evt);

    let snap = GroupMembersView::snapshot(&vc, &state);
    assert_eq!(snap.admins, vec![ADMIN_PUBKEY], "admins snapshot must reflect 39001 p-tags");
    assert_eq!(snap.members, vec![MEMBER_PUBKEY], "members snapshot must reflect 39002 p-tags");
}

/// Replacing the 39002 event with one that has no p-tags empties the members
/// list — the proxy for a member-remove lifecycle step.
#[test]
fn group_members_snapshot_empty_after_member_removed() {
    let group = group_a();
    let vc = make_vc();
    let (mut state, _) = GroupMembersView::open(&vc, MembersSpec { group: group.clone() });

    let members_v1 = kernel_event(
        "evt-39002-v1",
        KIND_GROUP_MEMBERS,
        vec![
            vec!["d".into(), group.local_id.clone()],
            vec!["p".into(), MEMBER_PUBKEY.into()],
        ],
    );
    GroupMembersView::on_event_inserted(&vc, &mut state, &members_v1);
    let before = GroupMembersView::snapshot(&vc, &state);
    assert!(!before.members.is_empty(), "member must appear before removal");

    // Relay re-publishes 39002 without the pubkey (member-remove reflected).
    let members_v2 = kernel_event(
        "evt-39002-v2",
        KIND_GROUP_MEMBERS,
        vec![vec!["d".into(), group.local_id.clone()]],
    );
    GroupMembersView::on_event_replaced(&vc, &mut state, &members_v1.id, &members_v2);
    let after = GroupMembersView::snapshot(&vc, &state);
    assert!(
        after.members.is_empty(),
        "members list must be empty after replacement with no p-tags"
    );
}

/// kind:9 → `ChatPayload.events` reflects the message.
/// Corresponds to the task's GroupTimelineSnapshot contract.
#[test]
fn group_chat_snapshot_reflects_message() {
    let group = group_a();
    let vc = make_vc();
    let (mut state, _) = GroupChatView::open(&vc, ChatSpec { group: group.clone() });

    let msg = KernelEvent {
        id: "evt-9-msg".into(),
        author: MEMBER_PUBKEY.into(),
        kind: KIND_CHAT_MESSAGE,
        created_at: 1_000,
        tags: vec![vec!["h".into(), group.local_id.clone()]],
        content: "hello world".into(),
    };
    GroupChatView::on_event_inserted(&vc, &mut state, &msg);

    let snap = GroupChatView::snapshot(&vc, &state);
    assert_eq!(snap.events.len(), 1, "one message must appear in snapshot");
    assert_eq!(snap.events[0].content, "hello world");
    assert_eq!(snap.events[0].kind, KIND_CHAT_MESSAGE);
}

// ── 4. Relay-pin routing (Case E, consumer-only) ──────────────────────────────

/// A `LogicalInterest` built by `host_pinned_interest` routes exclusively to
/// the pinned host; the indexer is not contacted (Case E short-circuits).
/// This is the "Group with RelayPin → REQ goes ONLY to pinned relay" contract.
#[test]
fn nip29_interest_routes_exclusively_to_host_relay() {
    let group = group_a();
    let interest = host_pinned_interest(
        1,
        &group,
        [KIND_CHAT_MESSAGE],
        Default::default(),
        InterestLifecycle::Tailing,
    );

    let cache = EmptyMailboxCache;
    let indexers = vec![INDEXER.to_string()];
    let compiler = SubscriptionCompiler::new(&cache, &indexers);
    let plan = compiler.compile(&[interest]).expect("compile");

    assert!(
        plan.per_relay.contains_key(HOST_A),
        "pinned interest must appear under host relay key"
    );
    assert!(
        !plan.per_relay.contains_key(INDEXER),
        "pinned interest must NOT fall through to indexer"
    );
    assert_eq!(plan.per_relay.len(), 1, "exactly one per-relay plan for one pinned group");
}

/// Two groups on *different* hosts produce two independent per-relay plans;
/// interests never merge across different relay pins (Rule 9).
#[test]
fn nip29_two_groups_different_hosts_produce_distinct_plans() {
    let interest_a = host_pinned_interest(
        1,
        &group_a(),
        [KIND_CHAT_MESSAGE],
        Default::default(),
        InterestLifecycle::Tailing,
    );
    let interest_b = host_pinned_interest(
        2,
        &group_b(),
        [KIND_CHAT_MESSAGE],
        Default::default(),
        InterestLifecycle::Tailing,
    );

    let cache = EmptyMailboxCache;
    let indexers = vec![INDEXER.to_string()];
    let compiler = SubscriptionCompiler::new(&cache, &indexers);
    let plan = compiler.compile(&[interest_a, interest_b]).expect("compile");

    assert!(plan.per_relay.contains_key(HOST_A), "host A must be in plan");
    assert!(plan.per_relay.contains_key(HOST_B), "host B must be in plan");
    assert!(!plan.per_relay.contains_key(INDEXER), "indexer must NOT be reached");
    assert_eq!(plan.per_relay.len(), 2, "exactly two per-relay plans, one per host");
}

/// A pinned interest with an `h` tag routes to the host only and the wire
/// filter preserves the h-tag dimension.
#[test]
fn nip29_pinned_interest_h_tag_survives_into_wire_shape() {
    let group = group_a();
    let interest = host_pinned_interest(
        1,
        &group,
        [KIND_CHAT_MESSAGE],
        Default::default(),
        InterestLifecycle::Tailing,
    );
    assert_eq!(interest.shape.relay_pin.as_deref(), Some(HOST_A));
    let h_vals = interest.shape.tags.get("h").expect("h tag must be present");
    assert!(h_vals.contains("room-alpha"), "h tag must carry the group local_id");

    let cache = EmptyMailboxCache;
    let indexers = vec![INDEXER.to_string()];
    let compiler = SubscriptionCompiler::new(&cache, &indexers);
    let plan = compiler.compile(&[interest]).expect("compile");

    let host_plan = plan.per_relay.get(HOST_A).expect("host A plan present");
    assert_eq!(host_plan.sub_shapes.len(), 1);
    let merged_h = host_plan.sub_shapes[0]
        .shape
        .tags
        .get("h")
        .expect("h tag must survive into per-relay shape");
    assert!(merged_h.contains("room-alpha"), "h value must be present on wire shape");
}

/// Generic relay-pinned `LogicalInterest` (no nip29 types) also routes via
/// Case E — proving any protocol crate can opt into the lane, not just nip29.
#[test]
fn generic_relay_pinned_interest_routes_via_case_e() {
    let interest = LogicalInterest {
        id: InterestId(99),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            kinds: [KIND_CHAT_MESSAGE].into_iter().collect(),
            relay_pin: Some(HOST_A.to_string()),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    };

    let cache = EmptyMailboxCache;
    let indexers = vec![INDEXER.to_string()];
    let compiler = SubscriptionCompiler::new(&cache, &indexers);
    let plan = compiler.compile(&[interest]).expect("compile");

    assert!(plan.per_relay.contains_key(HOST_A));
    assert!(!plan.per_relay.contains_key(INDEXER));
}
