//! Backend 2 — NIP-46 bunker resolves `SignerOp::Pending`: park → idle-loop
//! drain → continuation runs. Covers both the happy-path broker round-trip
//! and a broker rejection (D6 — no stuck spinner).

use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use nmp_signer_iface::{RemoteSignerHandle, SignedEvent, SignerError, SignerOp};

use super::signer_fixtures_support::{
    capture_continuation, draft_unsigned, fresh_identity, test_keys, PendingRemoteSigner,
};
use crate::actor::commands;
use crate::actor::pending_sign::{resolve_parked_op, ParkedOpSink};
use crate::actor::signer_port_test_harness::dispatch_one;
use crate::actor::{ActorCommand, SignCommand};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

#[test]
fn bunker_backend_parks_then_drain_invokes_continuation_with_signed_event() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Register a Pending remote signer as the active account. Keep an Arc to
    // its sign-count + the keys so we can mint the broker's eventual response.
    let keys = test_keys();
    let stub = PendingRemoteSigner::new(keys.clone());
    let sign_count = Arc::clone(&stub.sign_count);
    let stub_pk = stub.pubkey_hex();
    // We need the stub AFTER add_signer consumes it (to read `last_sender`), so
    // build the signed response up front from a parallel signer with the same
    // keys.
    let responder = PendingRemoteSigner::new(keys);

    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(stub)),
        true,
        false,
    );
    assert_eq!(identity.active_pubkey().as_deref(), Some(stub_pk.as_str()));

    let (captured, continuation) = capture_continuation();
    let unsigned = draft_unsigned(&stub_pk);

    let mut parked = dispatch_one(
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned: unsigned.clone(),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );

    // Bunker resolves Pending — the op is PARKED and the continuation has NOT
    // run yet (this is the asynchronous broker round-trip).
    assert_eq!(
        sign_count.load(Ordering::Relaxed),
        1,
        "stub signed exactly once"
    );
    assert_eq!(parked.len(), 1, "a Pending bunker op must be parked");
    assert!(
        captured.lock().unwrap().is_none(),
        "continuation must NOT run while the broker round-trip is pending"
    );

    // The broker turns the request around. Pull the receiver the parked op is
    // polling and feed it the signed event.
    let signed = responder.signed_for(&unsigned);
    {
        // Replace the parked op (inside the SignContinuation sink) with one whose
        // sender we control, then resolve it — mirrors a later idle-tick delivery.
        let ParkedOpSink::SignContinuation { op, .. } = &mut parked[0].sink else {
            panic!("expected a SignContinuation sink");
        };
        let (tx, rx): (
            Sender<Result<SignedEvent, SignerError>>,
            Receiver<Result<SignedEvent, SignerError>>,
        ) = channel();
        *op = SignerOp::Pending(rx);
        tx.send(Ok(signed.clone())).unwrap();
    }

    // First drain tick resolves it: the SAME continuation runs, now from the
    // idle-loop drain (not inline) — the worker code path is identical.
    let drained = resolve_parked_op(&mut parked[0], &mut kernel);
    assert!(
        !drained.keep,
        "a resolved op is dropped from the parked queue"
    );

    let outcome = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation must run from the drain once the broker responds");
    let got = outcome.expect("bunker sign must succeed");
    assert_eq!(got.unsigned.kind, 24242);
    assert_eq!(got.unsigned.content, "Upload blob");
    assert_eq!(got.unsigned.pubkey, stub_pk);

    // Same signature-verification rigour as the local path.
    let event_json = crate::actor::dispatch::signed_event_to_json(&got);
    let event: nostr::Event = serde_json::from_str(&event_json).expect("flat NIP-01 JSON");
    assert!(event.verify().is_ok(), "bunker signature must verify");
}

#[test]
fn bunker_backend_error_invokes_continuation_with_err_so_terminal_resolves() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = test_keys();
    let stub = PendingRemoteSigner::new(keys);
    let stub_pk = stub.pubkey_hex();
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(stub)),
        true,
        false,
    );

    let (captured, continuation) = capture_continuation();
    let mut parked = dispatch_one(
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned: draft_unsigned(&stub_pk),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );
    assert_eq!(parked.len(), 1, "Pending op parked");

    // Broker rejects the sign request.
    {
        let ParkedOpSink::SignContinuation { op, .. } = &mut parked[0].sink else {
            panic!("expected SignContinuation sink");
        };
        let (tx, rx) = channel::<Result<SignedEvent, SignerError>>();
        *op = SignerOp::Pending(rx);
        tx.send(Err(SignerError::Rejected("user declined".to_string())))
            .unwrap();
    }
    let drained = resolve_parked_op(&mut parked[0], &mut kernel);
    assert!(!drained.keep, "a rejected op is dropped");

    let outcome = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation must run on broker rejection (D6 — no stuck spinner)");
    let reason = outcome.expect_err("a rejection is an Err outcome");
    assert!(
        reason.contains("declined") || reason.to_lowercase().contains("reject"),
        "error reason should surface the broker rejection: {reason}"
    );
}
