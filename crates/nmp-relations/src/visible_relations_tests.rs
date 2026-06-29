use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::subs::SubScope;
use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_planner::{InterestLifecycle, InterestScope};

use super::*;
use crate::action::{
    VisibleNoteRelationsAction, VisibleNoteRelationsLifecycle, VisibleNoteRelationsModule,
};

const TARGET_KIND: u32 = nmp_kinds::KIND_SHORT_TEXT_NOTE;
const ADDRESS_KIND: u32 = nmp_kinds::KIND_LONG_FORM_ARTICLE;
const CONSUMER: &str = "feed-row-7";

fn event_id() -> String {
    "11".repeat(32)
}

fn address() -> String {
    format!("{}:{}:{}", ADDRESS_KIND, "22".repeat(32), "article")
}

fn action(target_kind: u32, target_address: Option<String>) -> VisibleNoteRelationsAction {
    VisibleNoteRelationsAction {
        lifecycle: VisibleNoteRelationsLifecycle::Claim,
        target_event_id: event_id(),
        target_kind,
        consumer_id: CONSUMER.to_string(),
        target_address,
    }
}

fn assert_lane(
    interests: &[VisibleNoteRelationInterest],
    lane: &str,
    kind: u32,
    tag: &str,
    value: &str,
) {
    let item = interests
        .iter()
        .find(|item| item.lane == lane)
        .unwrap_or_else(|| panic!("missing lane {lane}"));
    assert_eq!(item.interest.shape.kinds, [kind].into_iter().collect());
    assert_eq!(
        item.interest
            .shape
            .tags
            .get(tag)
            .and_then(|values| values.iter().next().map(String::as_str)),
        Some(value)
    );
    assert!(item.interest.shape.relay_pin.is_none());
    assert!(matches!(item.interest.scope, InterestScope::ActiveAccount));
    assert!(matches!(
        item.interest.lifecycle,
        InterestLifecycle::Tailing
    ));
    assert_eq!(item.identity.scope, SubScope::Global);
}

fn run_execute(action: VisibleNoteRelationsAction) -> Vec<ActorCommand> {
    let cmds = std::cell::RefCell::new(Vec::new());
    VisibleNoteRelationsModule
        .execute(&ActionContext::default(), action, "relations-cid", &|cmd| {
            cmds.borrow_mut().push(cmd)
        })
        .expect("execute must succeed for valid input");
    cmds.into_inner()
}

#[test]
fn kind1_target_claim_opens_relation_lanes_without_relay_pin() {
    let id = event_id();
    let interests = visible_note_relation_interests(&action(TARGET_KIND, None)).unwrap();
    assert_eq!(
        interests.iter().map(|item| item.lane).collect::<Vec<_>>(),
        vec!["replies", "reactions", "reposts", "zaps", "comments"]
    );
    assert_lane(
        &interests,
        "replies",
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        "e",
        &id,
    );
    assert_lane(&interests, "reactions", nmp_kinds::KIND_REACTION, "e", &id);
    assert_lane(&interests, "reposts", nmp_nip18::KIND_REPOST, "e", &id);
    assert_lane(&interests, "zaps", nmp_nip57::KIND_ZAP_RECEIPT, "e", &id);
    assert_lane(
        &interests,
        "comments",
        nmp_nip22::KIND_NIP22_COMMENT,
        "E",
        &id,
    );
}

#[test]
fn non_kind1_event_target_uses_nip22_for_replies_and_comments() {
    let id = event_id();
    let interests = visible_note_relation_interests(&action(ADDRESS_KIND, None)).unwrap();
    assert_eq!(interests.len(), 5);
    assert_lane(
        &interests,
        "replies",
        nmp_nip22::KIND_NIP22_COMMENT,
        "E",
        &id,
    );
    assert_lane(&interests, "reactions", nmp_kinds::KIND_REACTION, "e", &id);
    assert_lane(
        &interests,
        "reposts",
        nmp_nip18::KIND_GENERIC_REPOST,
        "e",
        &id,
    );
    assert_lane(&interests, "zaps", nmp_nip57::KIND_ZAP_RECEIPT, "e", &id);
    assert_lane(
        &interests,
        "comments",
        nmp_nip22::KIND_NIP22_COMMENT,
        "E",
        &id,
    );
}

#[test]
fn addressable_target_uses_address_root_where_protocol_supports_it() {
    let id = event_id();
    let addr = address();
    let interests = visible_note_relation_interests(&action(ADDRESS_KIND, Some(addr.clone())))
        .expect("address target builds");
    assert_lane(
        &interests,
        "replies",
        nmp_nip22::KIND_NIP22_COMMENT,
        "A",
        &addr,
    );
    assert_lane(&interests, "reactions", nmp_kinds::KIND_REACTION, "e", &id);
    assert_lane(
        &interests,
        "reposts",
        nmp_nip18::KIND_GENERIC_REPOST,
        "a",
        &addr,
    );
    assert_lane(&interests, "zaps", nmp_nip57::KIND_ZAP_RECEIPT, "a", &addr);
    assert_lane(
        &interests,
        "comments",
        nmp_nip22::KIND_NIP22_COMMENT,
        "A",
        &addr,
    );
}

#[test]
fn release_drops_the_same_interest_identities_claim_opens() {
    let claim = action(ADDRESS_KIND, Some(address()));
    let expected: Vec<_> = visible_note_relation_interests(&claim)
        .unwrap()
        .into_iter()
        .map(|item| item.identity)
        .collect();
    let mut release = claim;
    release.lifecycle = VisibleNoteRelationsLifecycle::Release;
    let cmds = run_execute(release);
    assert_eq!(cmds.len(), expected.len());
    for (cmd, identity) in cmds.iter().zip(expected.iter()) {
        let ActorCommand::Interests(InterestsCommand::DropInterestOwner(actual)) = cmd else {
            panic!("expected DropInterestOwner, got {cmd:?}");
        };
        assert_eq!(actual, identity);
    }
}

#[test]
fn claim_sends_ensure_interest_commands() {
    let cmds = run_execute(action(TARGET_KIND, None));
    assert_eq!(cmds.len(), 5);
    for cmd in cmds {
        assert!(
            matches!(
                cmd,
                ActorCommand::Interests(InterestsCommand::EnsureInterest { .. })
            ),
            "expected EnsureInterest, got {cmd:?}"
        );
    }
}

#[test]
fn different_consumers_share_slot_key_but_not_owner_key() {
    let mut a = action(TARGET_KIND, None);
    a.consumer_id = "row-a".to_string();
    let mut b = action(TARGET_KIND, None);
    b.consumer_id = "row-b".to_string();
    let a_interests = visible_note_relation_interests(&a).unwrap();
    let b_interests = visible_note_relation_interests(&b).unwrap();
    for (left, right) in a_interests.iter().zip(b_interests.iter()) {
        assert_eq!(left.lane, right.lane);
        assert_eq!(left.identity.key, right.identity.key);
        assert_ne!(left.identity.owner, right.identity.owner);
    }
}

#[test]
fn start_rejects_invalid_inputs() {
    let mut ctx = ActionContext::default();
    let mut invalid = action(TARGET_KIND, None);
    invalid.target_event_id = "not-hex".to_string();
    assert!(matches!(
        VisibleNoteRelationsModule.start(&mut ctx, invalid),
        Err(ActionRejection::Invalid(_))
    ));

    let mut empty_consumer = action(TARGET_KIND, None);
    empty_consumer.consumer_id = " ".to_string();
    assert!(matches!(
        VisibleNoteRelationsModule.start(&mut ctx, empty_consumer),
        Err(ActionRejection::Invalid(_))
    ));

    let wrong_address = action(TARGET_KIND, Some(address()));
    assert!(matches!(
        VisibleNoteRelationsModule.start(&mut ctx, wrong_address),
        Err(ActionRejection::Invalid(_))
    ));
}
