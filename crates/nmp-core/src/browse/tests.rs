//! Unit tests for the `nmp.browse_relay` ActionModule (V-52).
//!
//! These tests verify:
//! 1. Validation rejects invalid relay URLs and zero interest_id.
//! 2. `execute` for `Open` drops then ensures the scoped relay-pinned
//!    interest with `relay_pin = Some(url)` and the correct kind set.
//! 3. `execute` for `Close` sends `DropInterestOwner`.
//! 4. The relay-pinned interest only covers the scoped relay
//!    (relay_pin semantics already tested in nmp-planner; we verify the field
//!    is populated correctly here).

use std::sync::Mutex;

use super::*;
use crate::actor::ActorCommand;
use crate::actor::InterestsCommand;
use crate::planner::{InterestId, InterestLifecycle};
use crate::substrate::{ActionContext, ActionModule, ActionRejection};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn capture_commands(action: BrowseRelayAction) -> Vec<ActorCommand> {
    let captured = Mutex::new(Vec::new());
    let ctx = ActionContext::default();
    BrowseRelayModule
        .execute(&ctx, action, "test-corr", &|cmd| {
            captured.lock().unwrap().push(cmd);
        })
        .expect("execute must not fail for valid actions");
    captured.into_inner().unwrap()
}

fn open_action(relay: &str, kinds: Vec<u32>, id: u64) -> BrowseRelayAction {
    BrowseRelayAction::Open {
        relay_url: relay.to_string(),
        kinds,
        lifecycle: BrowseLifecycle::Tailing,
        interest_id: id,
    }
}

fn ensured_interest(cmds: &[ActorCommand]) -> &crate::planner::LogicalInterest {
    match cmds {
        [ActorCommand::Interests(InterestsCommand::DropInterestOwner(drop_identity)), ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest })] =>
        {
            assert_eq!(
                drop_identity, identity,
                "open must replace the same scoped owner"
            );
            interest
        }
        other => panic!("expected DropInterestOwner then EnsureInterest, got {other:?}"),
    }
}

// ─── Validation tests ────────────────────────────────────────────────────────

#[test]
fn start_rejects_non_relay_url() {
    let action = open_action("not-a-relay-url", vec![1], 42);
    let mut ctx = ActionContext::default();
    let result = BrowseRelayModule.start(&mut ctx, action);
    assert!(
        matches!(result, Err(ActionRejection::Invalid(_))),
        "non-relay-URL must be rejected"
    );
}

#[test]
fn start_rejects_zero_interest_id() {
    let action = open_action("wss://relay.example.com", vec![1], 0);
    let mut ctx = ActionContext::default();
    let result = BrowseRelayModule.start(&mut ctx, action);
    assert!(
        matches!(result, Err(ActionRejection::Invalid(_))),
        "interest_id = 0 must be rejected (sentinel value)"
    );
}

#[test]
fn start_accepts_valid_open() {
    let action = open_action("wss://relay.example.com", vec![1, 6], 1);
    let mut ctx = ActionContext::default();
    assert!(
        BrowseRelayModule.start(&mut ctx, action).is_ok(),
        "valid Open must be accepted"
    );
}

#[test]
fn start_accepts_close_unconditionally() {
    let action = BrowseRelayAction::Close { interest_id: 1 };
    let mut ctx = ActionContext::default();
    assert!(
        BrowseRelayModule.start(&mut ctx, action).is_ok(),
        "Close must always be accepted"
    );
}

// ─── Execute tests ───────────────────────────────────────────────────────────

#[test]
fn execute_open_sends_scoped_ensure_interest_with_relay_pin() {
    let relay = "wss://relay.damus.io";
    let cmds = capture_commands(open_action(relay, vec![1], 99));
    let interest = ensured_interest(&cmds);
    assert_eq!(
        interest.shape.relay_pin.as_deref(),
        Some(relay),
        "relay_pin must be set to the requested relay URL"
    );
    assert_eq!(
        interest.id,
        InterestId(99),
        "interest id must match the requested interest_id"
    );
    assert!(
        interest.shape.kinds.contains(&1u32),
        "kind 1 must be in the interest shape"
    );
}

#[test]
fn execute_open_tailing_sets_tailing_lifecycle() {
    let cmds = capture_commands(BrowseRelayAction::Open {
        relay_url: "wss://relay.example.com".to_string(),
        kinds: vec![1],
        lifecycle: BrowseLifecycle::Tailing,
        interest_id: 10,
    });
    let interest = ensured_interest(&cmds);
    assert_eq!(
        interest.lifecycle,
        InterestLifecycle::Tailing,
        "tailing lifecycle must be preserved"
    );
}

#[test]
fn execute_open_one_shot_sets_oneshot_lifecycle() {
    let cmds = capture_commands(BrowseRelayAction::Open {
        relay_url: "wss://relay.example.com".to_string(),
        kinds: vec![1],
        lifecycle: BrowseLifecycle::OneShot,
        interest_id: 11,
    });
    let interest = ensured_interest(&cmds);
    assert_eq!(
        interest.lifecycle,
        InterestLifecycle::OneShot,
        "one_shot lifecycle must be preserved"
    );
}

#[test]
fn execute_close_sends_drop_interest_owner() {
    let action = BrowseRelayAction::Close { interest_id: 99 };
    let cmds = capture_commands(action);
    assert_eq!(cmds.len(), 1, "Close must produce exactly one command");
    match &cmds[0] {
        ActorCommand::Interests(InterestsCommand::DropInterestOwner(identity)) => {
            assert_eq!(*identity, browse_identity(99));
        }
        other => panic!("expected DropInterestOwner, got {other:?}"),
    }
}

#[test]
fn execute_open_with_empty_kinds_produces_wildcard_shape() {
    // Empty kinds = wildcard subscription (any kind) — valid, caller's choice.
    let cmds = capture_commands(open_action("wss://relay.example.com", vec![], 5));
    let interest = ensured_interest(&cmds);
    assert!(
        interest.shape.kinds.is_empty(),
        "empty kinds must produce a wildcard interest shape"
    );
}

#[test]
fn relay_pin_not_in_scope_of_nip65_fan_out() {
    // Verify that the interest has NO authors and NO hints — confirming that
    // it cannot trigger the NIP-65 author-mailbox fan-out in the planner.
    // The planner's case_e_relay_pinned suppresses all four-lane dispatch
    // when relay_pin is Some(_); this test documents that expectation at
    // the construction level.
    let cmds = capture_commands(open_action("wss://relay.damus.io", vec![1], 7));
    let interest = ensured_interest(&cmds);
    assert!(
        interest.shape.authors.is_empty(),
        "browse interest must have no authors (relay_pin suppresses NIP-65)"
    );
    assert!(
        interest.hints.is_empty(),
        "browse interest must have no hints"
    );
}
