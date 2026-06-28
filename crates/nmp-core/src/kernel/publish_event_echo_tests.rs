//! Regression tests for relay EVENT echoes as publish acceptance proof.
//!
//! Direct NIP-20 OK frames remain the primary verdict path. These tests cover
//! the fallback for relays that store and later serve the event but do not
//! deliver the OK frame on the app's publish socket.

use crate::kernel::{Kernel, RelayFrame};
use crate::publish::PublishTarget;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_network::role::RelayRole;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

const RAW_WRITE_RELAY: &str = "wss://Relay.Echo.Test/";
const CANONICAL_WRITE_RELAY: &str = "wss://relay.echo.test";
const OTHER_RELAY: &str = "wss://other.echo.test";

fn signed_note(keys: &::nostr::Keys, content: &str, created_at: u64) -> SignedEvent {
    let event = ::nostr::EventBuilder::text_note(content)
        .custom_created_at(::nostr::Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("generated keys sign");

    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: event.kind.as_u16() as u32,
            tags: event
                .tags
                .iter()
                .map(|tag: &::nostr::Tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

fn event_frame(signed: &SignedEvent, sub_id: &str) -> String {
    let event_json: serde_json::Value =
        serde_json::from_str(&signed.to_nip01_json()).expect("signed event serializes");
    serde_json::json!(["EVENT", sub_id, event_json]).to_string()
}

fn corrupt_sig_event_frame(signed: &SignedEvent, sub_id: &str) -> String {
    let mut event_json: serde_json::Value =
        serde_json::from_str(&signed.to_nip01_json()).expect("signed event serializes");
    event_json["sig"] = serde_json::Value::String("0".repeat(128));
    serde_json::json!(["EVENT", sub_id, event_json]).to_string()
}

fn entry_for<'a>(kernel: &'a Kernel, event_id: &str) -> &'a crate::kernel::PublishQueueEntry {
    kernel
        .publish_queue_snapshot()
        .iter()
        .find(|entry| entry.event_id == event_id)
        .expect("publish queue entry exists")
}

#[test]
fn verified_same_relay_event_echo_settles_publish_without_ok_frame() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.seed_kind10002_for_test(&author, &[RAW_WRITE_RELAY]);
    let signed = signed_note(&keys, "relay echo publish proof", 1_700_000_000);

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert_eq!(outbound.len(), 1);
    assert_eq!(outbound[0].relay_url, CANONICAL_WRITE_RELAY);
    assert_eq!(entry_for(&kernel, &signed.id).status, "accepted_locally");

    let retry = kernel.handle_message(
        RelayRole::Content,
        RAW_WRITE_RELAY,
        RelayFrame::Text(event_frame(&signed, "feed-echo")),
    );
    assert!(retry.is_empty(), "an echo OK must not schedule a retry");

    let snap = kernel.publish_status_snapshot();
    assert!(
        snap.in_flight.is_empty(),
        "echo must evict the in-flight row"
    );
    assert_eq!(snap.recent_ok.len(), 1);
    assert_eq!(
        snap.recent_ok[0].accepted_by,
        vec![CANONICAL_WRITE_RELAY.to_string()]
    );
    assert!(snap.recent_errors.is_empty());

    let entry = entry_for(&kernel, &signed.id);
    assert_eq!(entry.status, "ok");
    assert_eq!(entry.relay_outcomes.len(), 1);
    assert_eq!(entry.relay_outcomes[0].relay_url, CANONICAL_WRITE_RELAY);
    assert_eq!(entry.relay_outcomes[0].status, "ok");
}

#[test]
fn invalid_event_echo_does_not_settle_publish() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.seed_kind10002_for_test(&author, &[RAW_WRITE_RELAY]);
    let signed = signed_note(&keys, "bad relay echo proof", 1_700_000_100);

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert_eq!(outbound.len(), 1);

    let _ = kernel.handle_message(
        RelayRole::Content,
        RAW_WRITE_RELAY,
        RelayFrame::Text(corrupt_sig_event_frame(&signed, "feed-echo")),
    );

    let snap = kernel.publish_status_snapshot();
    assert_eq!(snap.in_flight.len(), 1);
    assert!(snap.recent_ok.is_empty());
    assert!(snap.recent_errors.is_empty());
    assert_eq!(entry_for(&kernel, &signed.id).status, "accepted_locally");
}

#[test]
fn valid_echo_from_non_target_relay_is_noop_for_publish_verdict() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.seed_kind10002_for_test(&author, &[RAW_WRITE_RELAY]);
    let signed = signed_note(&keys, "wrong relay echo proof", 1_700_000_200);

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert_eq!(outbound.len(), 1);

    let _ = kernel.handle_message(
        RelayRole::Content,
        OTHER_RELAY,
        RelayFrame::Text(event_frame(&signed, "feed-echo")),
    );

    let snap = kernel.publish_status_snapshot();
    assert_eq!(snap.in_flight.len(), 1);
    assert!(snap.recent_ok.is_empty());
    assert!(snap.recent_errors.is_empty());
    assert_eq!(entry_for(&kernel, &signed.id).status, "accepted_locally");
}
