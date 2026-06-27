//! Contact-list command coverage for [`super::KernelReducer::apply_actor_command`].
//!
//! These tests prove browser/headless runtimes can execute the same Rust-owned
//! NIP-02 follow edit path native uses: loaded kind:3 baseline in, unsigned
//! replacement kind:3 sign request out.

use super::*;
use crate::actor::{ActorCommand, ContactsCommand};
use crate::store::{RawEvent, VerifiedEvent};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

const ACCOUNT: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const FOLLOW_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FOLLOW_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn seed_kind3(r: &mut KernelReducer, created_at: u64, tags: Vec<Vec<String>>) {
    let event = VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "11".repeat(32),
        pubkey: ACCOUNT.to_string(),
        created_at,
        kind: 3,
        tags,
        content: "preserved content".to_string(),
        sig: "22".repeat(64),
    });
    r.kernel
        .event_store_handle()
        .insert(event, &"wss://relay.example/".to_string(), 0)
        .expect("seed kind:3");
}

#[test]
fn contacts_unfollow_needs_sign_with_full_kind3_replacement() {
    let mut r = KernelReducer::new();
    let _ = r.set_active_account(ACCOUNT.to_string());
    r.set_clock_for_test(Arc::new(crate::kernel::clock::FixedClock(
        UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    )));
    seed_kind3(
        &mut r,
        1_700_000_000,
        vec![
            vec!["p".to_string(), FOLLOW_A.to_string(), "wss://a".to_string()],
            vec!["p".to_string(), FOLLOW_B.to_string(), "wss://b".to_string()],
            vec!["d".to_string(), "custom".to_string()],
        ],
    );

    let outcome = r.apply_actor_command(ActorCommand::Contacts(ContactsCommand::Unfollow {
        pubkey: FOLLOW_A.to_string(),
        correlation_id: Some("unfollow-cid".to_string()),
    }));

    let CommandApplyOutcome::NeedsSign {
        request,
        action_correlation_id,
        ..
    } = outcome
    else {
        panic!("expected NeedsSign, got {outcome:?}");
    };
    assert_eq!(request.account_pubkey, ACCOUNT);
    assert_eq!(action_correlation_id.as_deref(), Some("unfollow-cid"));

    let unsigned: serde_json::Value =
        serde_json::from_str(&request.unsigned_json).expect("unsigned JSON decodes");
    assert_eq!(unsigned["kind"], 3);
    assert_eq!(unsigned["content"], "preserved content");
    assert_eq!(unsigned["created_at"], 1_700_000_001);
    assert_eq!(
        unsigned["tags"],
        serde_json::json!([["p", FOLLOW_B, "wss://b"], ["d", "custom"]])
    );
}

#[test]
fn contacts_follow_fails_closed_until_kind3_loaded() {
    let mut r = KernelReducer::new();
    let _ = r.set_active_account(ACCOUNT.to_string());

    let outcome = r.apply_actor_command(ActorCommand::Contacts(ContactsCommand::Follow {
        pubkey: FOLLOW_A.to_string(),
        correlation_id: Some("follow-cid".to_string()),
    }));

    match outcome {
        CommandApplyOutcome::Unsupported { reason } => {
            assert_eq!(reason, "follow_list_not_loaded");
        }
        other => panic!("expected Unsupported(follow_list_not_loaded), got {other:?}"),
    }
}
