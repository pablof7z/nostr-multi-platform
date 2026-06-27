//! Timestamp coverage for browser/headless publish sign requests.
//!
//! Split from `command_apply_publish_tests.rs` to keep the publish command
//! coverage below the file-size hard cap.

use super::*;
use crate::actor::{ActorCommand, PublishCommand};
use nmp_signer_iface::UnsignedEvent;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

const ACCOUNT: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const RELAY: &str = "wss://relay.example";

#[test]
fn unsigned_event_zero_created_at_is_restamped_before_sign_roundtrip() {
    let mut r = KernelReducer::new();
    let _ = r.set_active_account(ACCOUNT.to_string());
    r.set_clock_for_test(Arc::new(crate::kernel::clock::FixedClock(
        UNIX_EPOCH + Duration::from_secs(1_700_000_123),
    )));

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::UnsignedEvent {
        event: UnsignedEvent {
            pubkey: String::new(),
            kind: 10_050,
            tags: vec![vec!["relay".to_string(), RELAY.to_string()]],
            content: String::new(),
            created_at: 0,
        },
        correlation_id: Some("unsigned-restamp-cid".to_string()),
        signer_pubkey: None,
    }));

    let CommandApplyOutcome::NeedsSign { request, .. } = outcome else {
        panic!("expected NeedsSign, got {outcome:?}");
    };
    let unsigned: serde_json::Value =
        serde_json::from_str(&request.unsigned_json).expect("unsigned JSON decodes");
    assert_eq!(
        unsigned["created_at"], 1_700_000_123,
        "browser reducer must honor the PublishCommand::UnsignedEvent D7 sentinel before signing"
    );
}
