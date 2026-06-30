//! Pre-chain failure oracles for DM send.

use super::*;

#[test]
fn no_active_account_toasts_and_records_failure() {
    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(
            "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee",
            RECIPIENT_HEX_PLACEHOLDER,
        ),
        recipient_pubkey: RECIPIENT_HEX_PLACEHOLDER.to_string(),
        correlation_id: Some("cid-no-account".to_string()),
    };
    let empty = EmptyDmInboxRelayLookup;
    let (rec, rx) = run_cmd(cmd, None, &empty, 1_700_000_000);

    // No chain launched — nothing on the channel.
    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    let toasts = rec.toasts.borrow();
    assert_eq!(toasts.len(), 1, "exactly one toast: the no-account message");
    assert!(
        toasts[0]
            .as_deref()
            .map(|s| s.contains("no active account"))
            .unwrap_or(false),
        "toast carries the no-account reason: {:?}",
        toasts[0]
    );
    let failures = rec.failures.borrow();
    assert_eq!(
        failures.len(),
        1,
        "D6 — exactly one Failed terminal recorded"
    );
    assert_eq!(failures[0].0, "cid-no-account");
}

#[test]
fn malformed_recipient_pubkey_toasts_and_records_failure() {
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, "not-a-pubkey"),
        recipient_pubkey: "not-a-pubkey".to_string(),
        correlation_id: Some("cid-bad-pubkey".to_string()),
    };
    let empty = EmptyDmInboxRelayLookup;
    let (rec, _rx) = run_cmd(cmd, Some(sender_hex), &empty, 1_700_000_000);

    let toasts = rec.toasts.borrow();
    assert!(
        toasts.iter().any(|t| t
            .as_deref()
            .map(|s| s.contains("recipient pubkey"))
            .unwrap_or(false)),
        "D6 — toast surfaces the malformed-pubkey reason: {toasts:?}"
    );
    let failures = rec.failures.borrow();
    assert_eq!(failures.len(), 1);
}

#[test]
fn missing_kind10050_for_recipient_fails_closed() {
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();

    let cache = Arc::new(DmRelayCache::new());
    // Seed the sender's relays; deliberately leave the recipient's missing.
    cache.upsert(
        sender_hex.clone(),
        vec!["wss://sender-dm.example".to_string()],
    );

    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: Some("cid-fail-closed".to_string()),
    };
    let (rec, rx) = run_cmd(cmd, Some(sender_hex), cache.as_ref(), 1_700_000_000);

    assert!(
        rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "fail-closed — no chain launched, no PublishSignedEvent"
    );
    let toasts = rec.toasts.borrow();
    assert!(
        toasts.iter().any(|t| t
            .as_deref()
            .map(|s| s.contains("kind:10050") && s.contains("recipient"))
            .unwrap_or(false)),
        "D10 — toast names kind:10050 + which envelope was blocked: {toasts:?}"
    );
}
