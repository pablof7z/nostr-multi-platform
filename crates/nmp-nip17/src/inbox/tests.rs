//! `DmInboxProjection` decrypt-independent unit tests.
//!
//! The port-driven gift-UNWRAP decrypt tests (ADR-0072 §D6) live in the sibling
//! `inbox/chain_tests.rs` — they need the `CommandSender` drain harness. This
//! file covers the parts that do not decrypt: snapshot shape, the kind filter,
//! the active-account interest, and serde round-tripping.

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use super::*;
use nmp_core::{ActorMail, CommandSender};
use nostr::Keys;

/// A projection whose active account is `keys`' pubkey. Decryption is exercised
/// in `chain_tests.rs`; here we only need a constructed projection, so the
/// command receiver is dropped.
fn inbox_for(keys: &Keys) -> DmInboxProjection {
    let (tx, _rx) = channel::<ActorMail>();
    let active = Arc::new(Mutex::new(Some(keys.public_key().to_hex())));
    DmInboxProjection::new(CommandSender::new(tx), active)
}

/// A projection with no active account (not signed in).
fn inbox_not_signed_in() -> DmInboxProjection {
    let (tx, _rx) = channel::<ActorMail>();
    DmInboxProjection::new(CommandSender::new(tx), Arc::new(Mutex::new(None)))
}

#[test]
fn fresh_inbox_yields_empty_snapshot() {
    // No active account → empty + decrypt_state "unavailable" (the host hides
    // the DM screen). ADR-0072 §D7: the tri-state replaced the old bool.
    let inbox = inbox_not_signed_in();
    let snap = inbox.snapshot();
    assert!(snap.conversations.is_empty());
    assert_eq!(
        snap.decrypt_state, "unavailable",
        "no active account → unavailable so the host hides the DM screen"
    );
    assert_eq!(snap.undecrypted_count, 0);
    assert_eq!(
        inbox.snapshot_json(),
        serde_json::json!({
            "conversations": [],
            "decrypt_state": "unavailable",
            "undecrypted_count": 0,
        })
    );
}

#[test]
fn signed_in_idle_inbox_is_ok() {
    // A signed-in account with no pending decrypts reports "ok" (§D7). A local
    // and a bunker account are identical at this seam when nothing is in flight.
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);
    let snap = inbox.snapshot();
    assert!(snap.conversations.is_empty());
    assert_eq!(
        snap.decrypt_state, "ok",
        "a signed-in account with nothing pending is decrypt-ok (§D7)"
    );
    assert_eq!(snap.undecrypted_count, 0);
}

#[test]
fn kind_filter_is_gift_wrap_only() {
    let filter = DmInboxProjection::kind_filter();
    assert!(
        filter.matches(KIND_GIFT_WRAP),
        "kind:1059 gift-wrap must match"
    );
    assert!(!filter.matches(14), "kind:14 must NOT match — it is sealed");
    assert!(!filter.matches(1), "plain notes must not match");
}

#[test]
fn clear_empties_the_snapshot() {
    // `clear()` drops messages and bumps the epoch. With no messages yet it is a
    // safe no-op that still leaves an empty snapshot.
    let bob = Keys::generate();
    let inbox = inbox_for(&bob);
    inbox.clear();
    assert!(inbox.snapshot().conversations.is_empty());
}

#[test]
fn active_giftwrap_interest_reuses_one_id_across_accounts() {
    let alice = active_giftwrap_inbox_interest("alice");
    let bob = active_giftwrap_inbox_interest("bob");
    assert_eq!(alice.id, bob.id, "account switch replaces one slot");
    assert_eq!(alice.id, active_giftwrap_inbox_interest_id());
    assert!(matches!(alice.scope, InterestScope::ActiveAccount));
    assert_eq!(alice.shape.p_tag_routing, PTagRouting::Nip17DmRelays);
    assert_eq!(bob.shape.p_tag_routing, PTagRouting::Nip17DmRelays);
    assert!(alice
        .shape
        .tags
        .get("p")
        .map(|s| s.contains("alice"))
        .unwrap_or(false));
    assert!(bob
        .shape
        .tags
        .get("p")
        .map(|s| s.contains("bob"))
        .unwrap_or(false));
}

#[test]
fn snapshot_round_trips_through_serde() {
    // A constructed snapshot serialises and deserialises losslessly.
    let snap = DmInboxSnapshot {
        conversations: vec![DmConversation {
            peer_pubkey: "a".repeat(64),
            messages: vec![DmMessage {
                id: "b".repeat(64),
                sender_pubkey: "c".repeat(64),
                content: "hi".to_string(),
                created_at: 100,
                reply_to: None,
                is_outgoing: false,
                source_relays: vec!["wss://r.example".to_string()],
            }],
        }],
        decrypt_state: "ok".to_string(),
        undecrypted_count: 0,
    };
    let encoded = serde_json::to_string(&snap).expect("serialises");
    let decoded: DmInboxSnapshot = serde_json::from_str(&encoded).expect("deserialises");
    assert_eq!(snap, decoded);
}
