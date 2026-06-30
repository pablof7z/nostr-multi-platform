//! Recipient publish-path oracles for DM send.

use super::*;

#[test]
fn happy_path_publishes_two_envelopes_pinned_to_kind10050_relays() {
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();

    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(
        sender_hex.clone(),
        vec!["wss://sender-dm.example".to_string()],
    );
    cache.upsert(
        recipient_hex.clone(),
        vec!["wss://recipient-dm.example".to_string()],
    );

    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: Some("cid-happy".to_string()),
    };
    let (rec, rx) = run_cmd(cmd, Some(sender_hex.clone()), cache.as_ref(), 1_700_000_000);
    let driver = ChainDriver::new(keys).run(&rx);

    let publishes = driver.publishes();
    assert_eq!(
        publishes.len(),
        2,
        "exactly two envelopes (recipient + self-copy)"
    );
    assert!(driver.toasts().is_empty(), "happy path — no toasts");
    assert!(
        driver.action_failures().is_empty(),
        "happy path — no Failed terminals"
    );
    assert!(rec.toasts.borrow().is_empty(), "no pre-chain toast either");

    let mut explicit_targets: Vec<(Vec<String>, Option<String>)> = Vec::new();
    for (raw, target, cid) in &publishes {
        assert_eq!(
            raw.kind, 1059,
            "the gift-wrap envelope is kind:1059, got {}",
            raw.kind
        );
        match target {
            PublishTarget::Explicit {
                relays,
                route_class: PublishRouteClass::VerifiedPrivateInbox,
            } => {
                explicit_targets.push(((*relays).clone(), (*cid).clone()));
            }
            other => {
                panic!("D10 — gift-wrap MUST route via PublishTarget::Explicit, got {other:?}")
            }
        }
    }

    // Relay sets must cover both receiver kind:10050 lists.
    let mut all_relays: Vec<String> = explicit_targets
        .iter()
        .flat_map(|(relays, _)| relays.clone())
        .collect();
    all_relays.sort();
    assert_eq!(
        all_relays,
        vec![
            "wss://recipient-dm.example".to_string(),
            "wss://sender-dm.example".to_string(),
        ],
        "recipient envelope pins to recipient's kind:10050; self-copy pins to sender's"
    );

    // Single-terminal invariant: only the recipient envelope carries the cid.
    let recipient_entry = explicit_targets
        .iter()
        .find(|(relays, _)| relays.contains(&"wss://recipient-dm.example".to_string()));
    let self_copy_entry = explicit_targets
        .iter()
        .find(|(relays, _)| relays.contains(&"wss://sender-dm.example".to_string()));
    assert_eq!(
        recipient_entry.map(|(_, cid)| cid.as_deref()),
        Some(Some("cid-happy")),
        "recipient envelope must carry the correlation_id for the action terminal"
    );
    assert_eq!(
        self_copy_entry.map(|(_, cid)| cid.as_deref()),
        Some(None),
        "self-copy envelope must carry None — its relay ack must not produce a second terminal"
    );
}

#[test]
fn recipient_envelope_round_trips_to_the_original_rumor() {
    // The recipient kind:1059 must unwrap (with the recipient's keys) back to
    // the kind:14 rumor — proving the chain assembled a real, decryptable seal.
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();
    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(sender_hex.clone(), vec!["wss://s.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://r.example".to_string()]);

    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: None,
    };
    let (_rec, rx) = run_cmd(cmd, Some(sender_hex.clone()), cache.as_ref(), 1_700_000_000);
    let driver = ChainDriver::new(keys.clone()).run(&rx);

    // The recipient envelope is the one pinned to the recipient's relay.
    let (recipient_raw, _, _) = driver
        .publishes()
        .into_iter()
        .find(|(_, target, _)| {
            matches!(target, PublishTarget::Explicit { relays, route_class: PublishRouteClass::VerifiedPrivateInbox } if relays.contains(&"wss://r.example".to_string()))
        })
        .expect("recipient envelope present");

    let envelope = raw_to_nostr_event(recipient_raw);
    let unwrapped =
        nmp_nip59::unwrap_gift_wrap(&recipient_keys, &envelope).expect("recipient can unwrap");
    assert_eq!(
        unwrapped.sender,
        keys.public_key(),
        "seal author is the sender"
    );
    assert_eq!(unwrapped.rumor.content, "hello over NIP-17");
    assert_eq!(u16::from(unwrapped.rumor.kind), 14);
}

#[test]
fn rumor_created_at_is_restamped_when_zero_sentinel() {
    // D7 — the host sends `created_at: 0`; the body re-stamps from `now_secs`
    // before sealing. We read it back by unwrapping the recipient envelope.
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();
    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(sender_hex.clone(), vec!["wss://s.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://r.example".to_string()]);

    let now: u64 = 1_700_000_777;
    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: None,
    };
    let (_rec, rx) = run_cmd(cmd, Some(sender_hex.clone()), cache.as_ref(), now);
    let driver = ChainDriver::new(keys.clone()).run(&rx);

    let (recipient_raw, _, _) = driver
        .publishes()
        .into_iter()
        .find(|(_, target, _)| {
            matches!(target, PublishTarget::Explicit { relays, route_class: PublishRouteClass::VerifiedPrivateInbox } if relays.contains(&"wss://r.example".to_string()))
        })
        .expect("recipient envelope present");
    let envelope = raw_to_nostr_event(recipient_raw);
    let unwrapped = nmp_nip59::unwrap_gift_wrap(&recipient_keys, &envelope).unwrap();
    assert_eq!(
        unwrapped.rumor.created_at.as_secs(),
        now,
        "D7 — the rumor's created_at is re-stamped from the kernel clock"
    );
}
