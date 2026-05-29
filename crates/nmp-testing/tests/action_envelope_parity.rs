//! Cross-platform action envelope parity tests.
//!
//! Calls the real production builder functions from `nmp_app_chirp::typed_api`
//! and asserts on the actual (namespace, JSON) output they produce.  These are
//! NOT tautological: the expected values were verified against the production
//! code in `apps/chirp/nmp-app-chirp/src/typed_api.rs`.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --test action_envelope_parity
//! ```

use nmp_app_chirp::typed_api::{
    follow_action, publish_note_action, react_action, send_dm_action, sign_in_nsec_action,
    switch_account_action, unfollow_action,
};

// ---------------------------------------------------------------------------
// publish_note_action
// ---------------------------------------------------------------------------

#[test]
fn publish_note_action_has_correct_namespace_and_content() {
    let (ns, json) = publish_note_action("hello world", None);
    assert_eq!(ns, "nmp.publish");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["PublishNote"]["content"], "hello world");
    // reply_to_id absent → JSON null
    assert!(v["PublishNote"]["reply_to_id"].is_null());
    // target is always present
    assert!(v["PublishNote"]["target"].is_string());
}

#[test]
fn publish_note_action_with_reply_has_reply_to_id() {
    let (ns, json) = publish_note_action("reply", Some("abc123"));
    assert_eq!(ns, "nmp.publish");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["PublishNote"]["content"], "reply");
    assert_eq!(v["PublishNote"]["reply_to_id"], "abc123");
}

// ---------------------------------------------------------------------------
// react_action
// ---------------------------------------------------------------------------

#[test]
fn react_action_has_correct_event_id_and_reaction() {
    let (ns, json) = react_action("eventabc", "+");
    assert_eq!(ns, "nmp.nip25.react");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["target_event_id"], "eventabc");
    assert_eq!(v["reaction"], "+");
}

// ---------------------------------------------------------------------------
// follow_action
// ---------------------------------------------------------------------------

#[test]
fn follow_action_has_correct_pubkey() {
    let (ns, json) = follow_action("pubkey123");
    assert_eq!(ns, "nmp.follow");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["pubkey"], "pubkey123");
}

// ---------------------------------------------------------------------------
// unfollow_action
// ---------------------------------------------------------------------------

#[test]
fn unfollow_action_has_correct_namespace_and_pubkey() {
    let (ns, json) = unfollow_action("pubkey456");
    assert_eq!(ns, "nmp.unfollow");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["pubkey"], "pubkey456");
}

// ---------------------------------------------------------------------------
// send_dm_action
// ---------------------------------------------------------------------------

#[test]
fn send_dm_action_has_correct_namespace_and_fields() {
    let (ns, json) = send_dm_action("recipientpubkey", "secret message");
    assert_eq!(ns, "nmp.nip17.send");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    // The key is recipient_pubkey, not recipient
    assert_eq!(v["recipient_pubkey"], "recipientpubkey");
    assert_eq!(v["content"], "secret message");
}

// ---------------------------------------------------------------------------
// sign_in_nsec_action
// ---------------------------------------------------------------------------

#[test]
fn sign_in_nsec_action_has_correct_namespace_and_nested_secret() {
    let (ns, json) = sign_in_nsec_action("nsec1abc");
    assert_eq!(ns, "nmp.sign_in_nsec");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    // secret is NESTED under SignInNsec
    assert_eq!(v["SignInNsec"]["secret"], "nsec1abc");
}

// ---------------------------------------------------------------------------
// switch_account_action
// ---------------------------------------------------------------------------

#[test]
fn switch_account_action_has_correct_namespace_and_pubkey() {
    let (ns, json) = switch_account_action("pubkey789");
    assert_eq!(ns, "nmp.switch_account");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["pubkey"], "pubkey789");
}
