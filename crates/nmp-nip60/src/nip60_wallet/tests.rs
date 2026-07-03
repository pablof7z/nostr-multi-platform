//! Unit tests for the wallet handle's core state: outbox queuing and
//! nutzap-redemption bookkeeping.

use super::*;
#[cfg(feature = "native")]
use crate::nutzap::ReceivedNutZap;

fn empty_wallet() -> Nip60WalletHandle {
    Nip60WalletHandle::create_new(&Keys::generate(), "https://mint.example").expect("wallet")
}

#[test]
fn create_new_queues_wallet_event_in_outbox() {
    let wallet = empty_wallet();
    let queued = wallet.take_outbox();
    assert_eq!(queued.len(), 1, "kind:17375 wallet event should be queued");
    // Outbox is drained on take.
    assert!(wallet.take_outbox().is_empty());
}

#[test]
fn redeemed_nutzap_ids_are_queryable() {
    let wallet = empty_wallet();
    let event_id = EventId::from_byte_array([3u8; 32]);

    wallet.mark_redeemed_nutzap(event_id);

    assert!(wallet.has_redeemed_nutzap(event_id));
    assert_eq!(wallet.redeemed_nutzap_ids(), vec![event_id]);
}

#[test]
#[cfg(feature = "native")]
fn redeem_nutzap_short_circuits_before_mint_for_known_event() {
    let wallet = empty_wallet();
    let event_id = EventId::from_byte_array([5u8; 32]);
    wallet.mark_redeemed_nutzap(event_id);

    let nutzap = ReceivedNutZap {
        event_id,
        sender_pubkey: Keys::generate().public_key(),
        proofs: Vec::new(),
        mint_url: "http://127.0.0.1:1".to_string(),
        amount_sats: 0,
        comment: String::new(),
        zapped_event_id: None,
    };

    let err = wallet.redeem_nutzap(&nutzap).expect_err("already redeemed");

    assert!(matches!(
        err,
        Nip60Error::AlreadyRedeemed(already_redeemed) if already_redeemed == event_id
    ));
}

/// Regression test for #2870 item 1: a wallet decoded from a kind:17375 that
/// carries a stale legacy `relay` tag must NOT leak that tag into the
/// kind:10019 it publishes. `publish_nutzap_info` must use only the
/// caller-supplied authoritative relay set.
#[test]
fn publish_nutzap_info_uses_caller_relays_not_legacy_hint() {
    let keys = Keys::generate();

    // Build a kind:17375 event carrying a stale legacy `relay` tag (as a
    // foreign/pre-fix wallet producer might).
    let config = crate::wallet_event::WalletConfig::generate(vec!["https://mint.example".into()]);
    let wallet_builder = crate::wallet_event::build_wallet_event(&config, &keys)
        .expect("build")
        .tag(nostr::Tag::custom(
            nostr::TagKind::custom("relay"),
            ["wss://stale-legacy-relay.example"],
        ));
    let wallet_event = wallet_builder.sign_with_keys(&keys).expect("sign");

    let wallet =
        Nip60WalletHandle::from_wallet_event(&keys, &wallet_event).expect("from_wallet_event");
    wallet.take_outbox(); // discard whatever from_wallet_event queued (nothing, today)

    let authoritative_relays = vec!["wss://real-authoritative-relay.example".to_string()];
    let event_id = wallet
        .publish_nutzap_info(&authoritative_relays)
        .expect("publish nutzap info");

    let queued = wallet.take_outbox();
    let published = queued
        .iter()
        .find(|e| e.id == event_id)
        .expect("kind:10019 event queued");
    let info = crate::nutzap::decode_nutzap_info_event(published);

    assert_eq!(info.relays, authoritative_relays);
    assert!(
        !info.relays.contains(&"wss://stale-legacy-relay.example".to_string()),
        "the legacy kind:17375 relay hint must never leak into the published kind:10019"
    );
}
