//! Port-failure oracles for the DM send continuation chain.

use super::*;

#[test]
fn recipient_encrypt_failure_surfaces_toast_and_action_failure() {
    // D6 — a recipient-chain port failure surfaces BOTH a toast and a
    // RecordActionFailure (the recipient envelope owns the action verdict).
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
        correlation_id: Some("cid-fail".to_string()),
    };
    let (_rec, rx) = run_cmd(cmd, Some(sender_hex), cache.as_ref(), 1_700_000_000);
    let driver = ChainDriver::new(keys).run_failing_encrypt(&rx, "broker rejected");

    assert!(
        driver.publishes().is_empty(),
        "no envelope published on failure"
    );
    assert!(
        driver
            .toasts()
            .iter()
            .any(|t| t.contains("recipient") && t.contains("broker rejected")),
        "D6 — toast names the recipient envelope + the reason: {:?}",
        driver.toasts()
    );
    let failures = driver.action_failures();
    assert_eq!(
        failures.len(),
        1,
        "recipient envelope records the action failure"
    );
    assert_eq!(failures[0].0, "cid-fail");
}

#[test]
fn self_copy_failure_surfaces_toast_only_not_action_failure() {
    // §D5 single-terminal — the recipient envelope SUCCEEDS, then the self-copy
    // chain fails. The self-copy failure surfaces a D6 toast but NO
    // RecordActionFailure: the recipient already got the message, so the action
    // promise is satisfied (the action verdict is the recipient envelope's).
    //
    // We drive: recipient encrypt → sign → publish (success, launching the
    // self-copy chain), then fail the self-copy's encrypt.
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
        correlation_id: Some("cid-selfcopy".to_string()),
    };
    let (_rec, rx) = run_cmd(cmd, Some(sender_hex.clone()), cache.as_ref(), 1_700_000_000);

    // Custom drive: resolve the recipient chain fully, then fail the self-copy's
    // first cipher step. The recipient is the FIRST envelope; the self-copy is
    // launched by the recipient's publish step.
    let mut driver = ChainDriver::new(keys);
    let mut recipient_done = false;
    while let Ok(mail) = rx.recv_timeout(Duration::from_millis(200)) {
        let ActorMail::Command(cmd) = mail else {
            unreachable!()
        };
        match cmd {
            ActorCommand::Sign(SignCommand::Nip44EncryptForAccount {
                peer_pubkey,
                plaintext,
                signer_pubkey,
                continuation,
            }) => {
                driver.pinned_signers.push(signer_pubkey);
                if recipient_done {
                    // This is the self-copy's encrypt — fail it.
                    continuation.call(Err("self-copy broker down".to_string()));
                } else {
                    let peer = nostr::PublicKey::parse(&peer_pubkey).unwrap();
                    let ct = nip44::encrypt(
                        driver.signer_keys.secret_key(),
                        &peer,
                        &plaintext,
                        Nip44Version::V2,
                    )
                    .unwrap();
                    continuation.call(Ok(ct));
                }
            }
            ActorCommand::Sign(SignCommand::EventForAccount {
                unsigned,
                signer_pubkey,
                continuation,
            }) => {
                driver.pinned_signers.push(signer_pubkey);
                let signed = driver.sign_seal(&unsigned);
                continuation.call(Ok(signed));
            }
            ActorCommand::Publish(PublishCommand::SignedEvent { .. }) => {
                // The recipient publish — record it and mark recipient done so
                // the next encrypt (self-copy) is failed.
                recipient_done = true;
                driver.terminals.push(cmd);
            }
            terminal => driver.terminals.push(terminal),
        }
    }

    // Exactly ONE publish (recipient) — the self-copy failed before publishing.
    assert_eq!(
        driver.publishes().len(),
        1,
        "recipient published; self-copy did not"
    );
    // A toast names the self-copy failure (D6 visibility) ...
    assert!(
        driver.toasts().iter().any(|t| t.contains("self-copy")),
        "D6 — self-copy failure surfaces a toast: {:?}",
        driver.toasts()
    );
    // ... but NO action failure (single-terminal — recipient got the message).
    assert!(
        driver.action_failures().is_empty(),
        "§D5 single-terminal — a self-copy failure must NOT record an action failure"
    );
}
