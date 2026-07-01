use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::actor::nip44_decrypt_session_port::{
    Nip44DecryptBatchItemPortOutcome, Nip44DecryptBatchPortOutcome, Nip44DecryptBatchPortResult,
    Nip44DecryptSessionBeginPortResult,
};
use nmp_core::actor::{ActorCommand, SignCommand};
use nmp_core::{ActorMail, CommandSender};
use nmp_signer_iface::{Nip44DecryptSessionGrant, NMP_NIP44_BACKFILL_SCOPE};
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use nostr::{EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};

use super::DmInboxProjection;

#[test]
fn store_backfill_uses_session_batches_and_files_decrypted_messages() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let event_store = nmp_core::slots::new_event_store_slot();
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    *event_store.lock().unwrap() = Some(Arc::clone(&store));

    for i in 0..3u64 {
        let envelope = gift_wrapped_dm(
            &alice,
            &bob.public_key(),
            &format!("batch message {i}"),
            1_700_000_000 + i,
        );
        store
            .insert(
                verified(&envelope),
                &"wss://dm-relay.example".to_string(),
                1_700_000_000_000 + i,
            )
            .expect("insert gift wrap");
    }

    let (projection, rx) = projection_for(&bob.public_key());
    assert!(projection.launch_batch_backfill(&event_store));
    assert_eq!(projection.snapshot().decrypt_state, "limited");
    assert_eq!(projection.snapshot().undecrypted_count, 3);

    let SignCommand::Nip44DecryptSessionBegin {
        request,
        signer_pubkey,
        continuation,
    } = recv_sign_command(&rx)
    else {
        panic!("expected decrypt-session begin");
    };
    assert_eq!(request.scope, NMP_NIP44_BACKFILL_SCOPE);
    assert_eq!(request.requester_pubkey, bob.public_key().to_hex());
    assert_eq!(request.max_items, 6);
    assert_eq!(request.expires_at, 0, "actor dispatch owns clock stamping");
    assert_eq!(
        signer_pubkey.as_deref(),
        Some(bob.public_key().to_hex().as_str())
    );
    continuation.call(Ok(Nip44DecryptSessionBeginPortResult::Granted(
        Nip44DecryptSessionGrant {
            session_id: "session-1".to_string(),
            max_batch_items: 8,
            expires_at: 1_700_000_300,
        },
    )));

    let SignCommand::Nip44DecryptBatch {
        request,
        signer_pubkey,
        continuation,
    } = recv_sign_command(&rx)
    else {
        panic!("expected outer decrypt batch");
    };
    assert_eq!(request.session_id, "session-1");
    assert_eq!(request.items.len(), 3);
    assert_eq!(
        signer_pubkey.as_deref(),
        Some(bob.public_key().to_hex().as_str())
    );
    continuation.call(Ok(Nip44DecryptBatchPortResult::Batch(decrypt_batch(
        &bob,
        request.items,
    ))));

    let SignCommand::Nip44DecryptBatch {
        request,
        signer_pubkey,
        continuation,
    } = recv_sign_command(&rx)
    else {
        panic!("expected inner decrypt batch");
    };
    assert_eq!(request.session_id, "session-1");
    assert_eq!(request.items.len(), 3);
    assert_eq!(
        signer_pubkey.as_deref(),
        Some(bob.public_key().to_hex().as_str())
    );
    continuation.call(Ok(Nip44DecryptBatchPortResult::Batch(decrypt_batch(
        &bob,
        request.items,
    ))));

    let SignCommand::Nip44DecryptSessionEnd {
        request,
        signer_pubkey,
        ..
    } = recv_sign_command(&rx)
    else {
        panic!("expected decrypt-session end");
    };
    assert_eq!(request.session_id, "session-1");
    assert_eq!(
        signer_pubkey.as_deref(),
        Some(bob.public_key().to_hex().as_str())
    );
    assert!(
        rx.try_recv().is_err(),
        "batch replay should be fully drained"
    );

    let snapshot = projection.snapshot();
    assert_eq!(snapshot.decrypt_state, "ok");
    assert_eq!(snapshot.undecrypted_count, 0);
    assert_eq!(snapshot.conversations.len(), 1);
    assert_eq!(
        snapshot.conversations[0].peer_pubkey,
        alice.public_key().to_hex()
    );
    let contents: Vec<&str> = snapshot.conversations[0]
        .messages
        .iter()
        .map(|msg| msg.content.as_str())
        .collect();
    assert_eq!(
        contents,
        vec!["batch message 0", "batch message 1", "batch message 2"]
    );
    assert_eq!(
        snapshot.conversations[0].messages[0].source_relays,
        vec!["wss://dm-relay.example".to_string()],
        "store replay preserves the event-store provenance relay"
    );
}

fn projection_for(pubkey: &PublicKey) -> (DmInboxProjection, Receiver<ActorMail>) {
    let (tx, rx) = channel::<ActorMail>();
    let active = Arc::new(Mutex::new(Some(pubkey.to_hex())));
    (DmInboxProjection::new(CommandSender::new(tx), active), rx)
}

fn recv_command(rx: &Receiver<ActorMail>) -> ActorCommand {
    match rx.try_recv().expect("expected actor mail") {
        ActorMail::Command(cmd) => cmd,
        other => panic!("expected ActorMail::Command, got {other:?}"),
    }
}

fn recv_sign_command(rx: &Receiver<ActorMail>) -> SignCommand {
    loop {
        match recv_command(rx) {
            ActorCommand::Sign(cmd) => return cmd,
            ActorCommand::Interests(_) => {
                // Storing a received DM hydrates that peer's kind:10050 relay
                // list. It is a normal side effect, not part of the decrypt
                // session sequence this test asserts.
            }
            other => panic!("unexpected actor command while waiting for sign command: {other:?}"),
        }
    }
}

fn decrypt_batch(
    recipient: &Keys,
    items: Vec<nmp_signer_iface::Nip44DecryptBatchItem>,
) -> Nip44DecryptBatchPortOutcome {
    let items = items
        .into_iter()
        .map(|item| {
            let peer = PublicKey::from_hex(&item.peer_pubkey).expect("peer pubkey");
            let plaintext =
                nostr::nips::nip44::decrypt(recipient.secret_key(), &peer, &item.ciphertext)
                    .expect("batch decrypt");
            Nip44DecryptBatchItemPortOutcome::Plaintext {
                id: item.id,
                plaintext,
            }
        })
        .collect();
    Nip44DecryptBatchPortOutcome { items }
}

fn gift_wrapped_dm(
    sender: &Keys,
    receiver: &PublicKey,
    content: &str,
    created_at: u64,
) -> nostr::Event {
    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(vec![Tag::public_key(*receiver)])
        .custom_created_at(Timestamp::from(created_at))
        .build(sender.public_key());
    nmp_nip59::gift_wrap_local(sender, receiver, &rumor, Timestamp::from(created_at))
        .expect("gift wrap succeeds")
}

fn verified(ev: &nostr::Event) -> VerifiedEvent {
    let raw = RawEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    };
    VerifiedEvent::try_from_raw(raw).expect("real signed event must verify")
}
