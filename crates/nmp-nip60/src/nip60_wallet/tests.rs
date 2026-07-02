//! Unit tests for the wallet handle's core state: outbox queuing and
//! nutzap-redemption bookkeeping.

use super::*;
use crate::nutzap::ReceivedNutZap;

fn empty_wallet() -> Nip60WalletHandle {
    Nip60WalletHandle::create_new(&Keys::generate(), "https://mint.example", Vec::new())
        .expect("wallet")
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
