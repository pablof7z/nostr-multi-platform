//! #1259 conformance: NIP-46 batch-capable bunker backfills NIP-17 DMs.
//!
//! This test stitches together the coverage that lower layers already prove in
//! isolation: stored kind:1059 replay from `nmp-nip17`, actor-lane NIP-46
//! transport from `nmp-nip46-runtime`, and a real `Nip46Signer` handle. The
//! mock bunker decrypts the NMP extension methods on the wire; the test never
//! calls `Nip46Signer::ingest_rpc_response` directly.

mod common;

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nmp_core::actor::nip44_decrypt_session_port::{
    Nip44DecryptBatchItemPortOutcome, Nip44DecryptBatchPortOutcome, Nip44DecryptBatchPortResult,
    Nip44DecryptSessionBeginPortResult, Nip44DecryptSessionEndPortResult,
};
use nmp_core::actor::{ActorCommand, IdentityCommand, SignCommand};
use nmp_core::{ActorMail, CommandSender};
use nmp_nip17::DmInboxProjection;
use nmp_signer_iface::{
    Nip44DecryptBatchItemResult, Nip44DecryptBatchResult, RemoteSignerHandle, SignerError,
    SignerOp, NMP_NIP44_DECRYPT_BATCH, NMP_NIP44_DECRYPT_SESSION_BEGIN,
    NMP_NIP44_DECRYPT_SESSION_END,
};
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use nostr::{EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};

use common::broker_adapter::broker_for_actor;
use common::mock_bunker_relay::MockBunkerRelay;

#[test]
fn capable_bunker_batch_backfill_over_nip46_transport_sets_decrypt_state_ok() {
    let bunker_keys = Keys::generate();
    let bob = Keys::generate();
    let alice = Keys::generate();
    let mock = MockBunkerRelay::spawn(bunker_keys.clone(), bob.clone())
        .expect("mock bunker relay must spawn");

    let (actor_tx, actor_rx) = mpsc::channel::<ActorMail>();
    let broker = broker_for_actor(CommandSender::new(actor_tx.clone()));
    let bunker_uri = format!(
        "bunker://{}?relay={}",
        bunker_keys.public_key().to_hex(),
        mock.ws_url()
    );
    broker.start_handshake(bunker_uri);

    let signer = wait_for_add_remote_signer(&actor_rx, Duration::from_secs(10))
        .expect("mock bunker handshake must add a remote signer");
    assert_eq!(signer.pubkey_hex(), bob.public_key().to_hex());

    let event_store = nmp_core::slots::new_event_store_slot();
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    *event_store.lock().unwrap() = Some(Arc::clone(&store));
    for i in 0..3_u64 {
        let envelope = gift_wrapped_dm(
            &alice,
            &bob.public_key(),
            &format!("transport batch message {i}"),
            1_700_010_000 + i,
        );
        store
            .insert(
                verified(&envelope),
                &"wss://dm-relay.example".to_string(),
                1_700_010_000_000 + i,
            )
            .expect("insert gift wrap");
    }

    let active = Arc::new(Mutex::new(Some(bob.public_key().to_hex())));
    let projection = DmInboxProjection::new(CommandSender::new(actor_tx), active);
    assert!(projection.launch_batch_backfill_for_test(&event_store));
    assert_eq!(projection.snapshot().decrypt_state, "limited");
    assert_eq!(projection.snapshot().undecrypted_count, 3);

    for _ in 0..4 {
        resolve_next_sign_command(&*signer, &actor_rx);
    }

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
        vec![
            "transport batch message 0",
            "transport batch message 1",
            "transport batch message 2",
        ]
    );

    let methods = mock.observed_methods();
    for method in [
        "connect",
        "get_public_key",
        NMP_NIP44_DECRYPT_SESSION_BEGIN,
        NMP_NIP44_DECRYPT_SESSION_END,
    ] {
        assert!(
            methods.iter().any(|seen| seen == method),
            "mock bunker must observe {method}; got {methods:?}"
        );
    }
    assert_eq!(
        methods
            .iter()
            .filter(|seen| seen.as_str() == NMP_NIP44_DECRYPT_BATCH)
            .count(),
        2,
        "outer and inner decrypt batches must cross the NIP-46 wire"
    );

    broker.cancel();
}

fn resolve_next_sign_command(signer: &dyn RemoteSignerHandle, rx: &Receiver<ActorMail>) {
    match recv_sign_command(rx, Duration::from_secs(10)) {
        SignCommand::Nip44DecryptSessionBegin {
            request,
            continuation,
            ..
        } => {
            let op = signer.nip44_decrypt_session_begin(request);
            continuation.call(map_begin(wait_for_op(signer, op, rx)));
        }
        SignCommand::Nip44DecryptBatch {
            request,
            continuation,
            ..
        } => {
            let expected_ids = request
                .items
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            let op = signer.nip44_decrypt_batch(request);
            continuation.call(map_batch(wait_for_op(signer, op, rx), &expected_ids));
        }
        SignCommand::Nip44DecryptSessionEnd {
            request,
            continuation,
            ..
        } => {
            let op = signer.nip44_decrypt_session_end(request);
            continuation.call(map_end(wait_for_op(signer, op, rx)));
        }
        other => panic!("unexpected sign command during decrypt-session backfill: {other:?}"),
    }
}

fn wait_for_op<T: Send + 'static>(
    signer: &dyn RemoteSignerHandle,
    mut op: SignerOp<T>,
    rx: &Receiver<ActorMail>,
) -> Result<T, SignerError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = op.poll() {
            return result;
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("signer operation timed out");
        match rx.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(ActorMail::Command(ActorCommand::Identity(
                IdentityCommand::DeliverSignerResponse { response_json },
            ))) => signer.deliver_response(&response_json),
            Ok(ActorMail::Command(ActorCommand::Identity(
                IdentityCommand::BunkerHandshakeProgress { stage, message, .. },
            ))) if stage == "failed" => panic!("bunker failed during op: {message:?}"),
            Ok(ActorMail::Command(ActorCommand::Interests(_))) => {}
            Ok(other) => panic!("unexpected actor mail while waiting for signer op: {other:?}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(SignerError::Backend(
                    "actor channel disconnected while waiting for signer op".to_string(),
                ));
            }
        }
    }
}

fn recv_sign_command(rx: &Receiver<ActorMail>, timeout: Duration) -> SignCommand {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for sign command");
        match rx.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(ActorMail::Command(ActorCommand::Sign(cmd))) => return cmd,
            Ok(ActorMail::Command(ActorCommand::Interests(_))) => {}
            Ok(ActorMail::Command(ActorCommand::Identity(
                IdentityCommand::BunkerHandshakeProgress { stage, message, .. },
            ))) if stage == "failed" => panic!("bunker handshake failed: {message:?}"),
            Ok(other) => panic!("unexpected actor mail while waiting for sign command: {other:?}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("actor channel disconnected"),
        }
    }
}

fn wait_for_add_remote_signer(
    rx: &Receiver<ActorMail>,
    timeout: Duration,
) -> Option<Box<dyn RemoteSignerHandle>> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        match rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(ActorMail::Command(ActorCommand::Identity(IdentityCommand::AddSigner {
                source: nmp_core::SignerSource::RemoteHandle(handle),
                ..
            }))) => return Some(handle),
            Ok(ActorMail::Command(ActorCommand::Identity(
                IdentityCommand::BunkerHandshakeProgress { stage, message, .. },
            ))) if stage == "failed" => panic!("bunker handshake failed: {message:?}"),
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

fn map_begin(
    result: Result<nmp_signer_iface::Nip44DecryptSessionGrant, SignerError>,
) -> Result<Nip44DecryptSessionBeginPortResult, String> {
    match result {
        Ok(grant) => Ok(Nip44DecryptSessionBeginPortResult::Granted(grant)),
        Err(SignerError::Unsupported(reason)) => {
            Ok(Nip44DecryptSessionBeginPortResult::Unsupported { reason })
        }
        Err(e) => Err(e.to_string()),
    }
}

fn map_batch(
    result: Result<Nip44DecryptBatchResult, SignerError>,
    expected_ids: &[String],
) -> Result<Nip44DecryptBatchPortResult, String> {
    match result {
        Ok(batch) => validate_batch(batch, expected_ids).map(Nip44DecryptBatchPortResult::Batch),
        Err(SignerError::Unsupported(reason)) => {
            Ok(Nip44DecryptBatchPortResult::Unsupported { reason })
        }
        Err(e) => Err(e.to_string()),
    }
}

fn map_end(result: Result<bool, SignerError>) -> Result<Nip44DecryptSessionEndPortResult, String> {
    match result {
        Ok(acknowledged) => Ok(Nip44DecryptSessionEndPortResult::Ended { acknowledged }),
        Err(SignerError::Unsupported(reason)) => {
            Ok(Nip44DecryptSessionEndPortResult::Unsupported { reason })
        }
        Err(e) => Err(e.to_string()),
    }
}

fn validate_batch(
    batch: Nip44DecryptBatchResult,
    expected_ids: &[String],
) -> Result<Nip44DecryptBatchPortOutcome, String> {
    let mut by_id = BTreeMap::<String, Nip44DecryptBatchItemResult>::new();
    for item in batch.items {
        if by_id.insert(item.id.clone(), item).is_some() {
            return Err("duplicate batch item id".to_string());
        }
    }
    if by_id.len() != expected_ids.len() {
        return Err(format!(
            "batch item count mismatch: got {}, expected {}",
            by_id.len(),
            expected_ids.len()
        ));
    }
    let mut items = Vec::with_capacity(expected_ids.len());
    for id in expected_ids {
        let item = by_id
            .get(id)
            .ok_or_else(|| format!("missing batch item id {id}"))?;
        match (&item.plaintext, &item.error) {
            (Some(plaintext), None) => items.push(Nip44DecryptBatchItemPortOutcome::Plaintext {
                id: id.clone(),
                plaintext: plaintext.clone(),
            }),
            (None, Some(error)) => items.push(Nip44DecryptBatchItemPortOutcome::Failed {
                id: id.clone(),
                error: error.clone(),
            }),
            _ => return Err(format!("malformed batch item id {id}")),
        }
    }
    Ok(Nip44DecryptBatchPortOutcome { items })
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
