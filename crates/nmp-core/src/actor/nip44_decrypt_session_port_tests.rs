//! Stage 2 of #1259: actor/core NIP-44 decrypt-session port tests.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_signer_iface::{
    Nip44DecryptBatchItem, Nip44DecryptBatchItemResult, Nip44DecryptBatchRequest,
    Nip44DecryptBatchResult, Nip44DecryptSessionBeginRequest, Nip44DecryptSessionEndRequest,
    Nip44DecryptSessionGrant, RemoteSignerHandle, SignedEvent, SignerError, SignerOp,
    UnsignedEvent,
};
use nostr::Keys;

use super::super::commands::{self, IdentityRuntime};
use super::super::pending_sign::resolve_parked_op;
use super::super::signer_port_test_harness::dispatch_one;
use super::super::{ActorCommand, IdentityCommand, SignCommand};
use super::{
    Nip44DecryptBatchContinuation, Nip44DecryptBatchItemPortOutcome, Nip44DecryptBatchPortOutcome,
    Nip44DecryptBatchPortResult, Nip44DecryptSessionBeginContinuation,
    Nip44DecryptSessionBeginPortResult, Nip44DecryptSessionEndContinuation,
    Nip44DecryptSessionEndPortResult,
};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

type BeginCapture = Arc<Mutex<Option<Result<Nip44DecryptSessionBeginPortResult, String>>>>;
type BatchCapture = Arc<Mutex<Option<Result<Nip44DecryptBatchPortResult, String>>>>;
type EndCapture = Arc<Mutex<Option<Result<Nip44DecryptSessionEndPortResult, String>>>>;

fn fresh_identity() -> IdentityRuntime {
    IdentityRuntime::new(
        commands::new_bunker_handshake_slot(),
        commands::new_signer_state_slot(),
    )
}

fn grant() -> Nip44DecryptSessionGrant {
    Nip44DecryptSessionGrant {
        session_id: "session-secret".to_string(),
        max_batch_items: 16,
        expires_at: 42,
    }
}

fn begin_request() -> Nip44DecryptSessionBeginRequest {
    Nip44DecryptSessionBeginRequest {
        scope: nmp_signer_iface::NMP_NIP44_BACKFILL_SCOPE.to_string(),
        requester_pubkey: "11".repeat(32),
        max_items: 2,
        expires_at: 42,
    }
}

fn batch_request() -> Nip44DecryptBatchRequest {
    Nip44DecryptBatchRequest {
        session_id: "session-secret".to_string(),
        items: vec![
            Nip44DecryptBatchItem {
                id: "item-a".to_string(),
                peer_pubkey: "22".repeat(32),
                ciphertext: "cipher-a".to_string(),
            },
            Nip44DecryptBatchItem {
                id: "item-b".to_string(),
                peer_pubkey: "33".repeat(32),
                ciphertext: "cipher-b".to_string(),
            },
        ],
    }
}

fn end_request() -> Nip44DecryptSessionEndRequest {
    Nip44DecryptSessionEndRequest {
        session_id: "session-secret".to_string(),
    }
}

fn capture_begin() -> (BeginCapture, Nip44DecryptSessionBeginContinuation) {
    let captured = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&captured);
    (
        captured,
        Nip44DecryptSessionBeginContinuation::new(move |o| {
            *slot.lock().unwrap() = Some(o);
        }),
    )
}

fn capture_batch() -> (BatchCapture, Nip44DecryptBatchContinuation) {
    let captured = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&captured);
    (
        captured,
        Nip44DecryptBatchContinuation::new(move |o| {
            *slot.lock().unwrap() = Some(o);
        }),
    )
}

fn capture_end() -> (EndCapture, Nip44DecryptSessionEndContinuation) {
    let captured = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&captured);
    (
        captured,
        Nip44DecryptSessionEndContinuation::new(move |o| {
            *slot.lock().unwrap() = Some(o);
        }),
    )
}

#[derive(Default)]
struct PendingSlots {
    begin: Mutex<Option<Sender<Result<Nip44DecryptSessionGrant, SignerError>>>>,
    batch: Mutex<Option<Sender<Result<Nip44DecryptBatchResult, SignerError>>>>,
    end: Mutex<Option<Sender<Result<bool, SignerError>>>>,
}

#[derive(Default)]
struct ReadySlots {
    begin: Mutex<Option<Result<Nip44DecryptSessionGrant, SignerError>>>,
    batch: Mutex<Option<Result<Nip44DecryptBatchResult, SignerError>>>,
    end: Mutex<Option<Result<bool, SignerError>>>,
}

struct SessionSignerState {
    pk: String,
    timeout: Duration,
    ready: ReadySlots,
    pending: PendingSlots,
}

impl SessionSignerState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            pk: Keys::generate().public_key().to_hex(),
            timeout: Duration::from_secs(5),
            ready: ReadySlots::default(),
            pending: PendingSlots::default(),
        })
    }

    fn deliver_ok(&self) {
        if let Some(tx) = self.pending.begin.lock().unwrap().take() {
            let _ = tx.send(Ok(grant()));
        }
        if let Some(tx) = self.pending.batch.lock().unwrap().take() {
            let _ = tx.send(Ok(success_batch()));
        }
        if let Some(tx) = self.pending.end.lock().unwrap().take() {
            let _ = tx.send(Ok(true));
        }
    }
}

#[derive(Clone)]
struct SessionSigner {
    state: Arc<SessionSignerState>,
}

impl std::fmt::Debug for SessionSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionSigner").finish_non_exhaustive()
    }
}

impl RemoteSignerHandle for SessionSigner {
    fn pubkey_hex(&self) -> String {
        self.state.pk.clone()
    }
    fn signer_kind(&self) -> &'static str {
        "nip46"
    }
    fn op_timeout(&self) -> Duration {
        self.state.timeout
    }
    fn sign(&self, _unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        SignerOp::err(SignerError::Backend("unused".to_string()))
    }
    fn nip44_encrypt(&self, _recipient_pubkey: &str, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Backend("unused".to_string()))
    }
    fn nip44_decrypt(&self, _sender_pubkey: &str, _ciphertext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Backend("unused".to_string()))
    }
    fn nip44_decrypt_session_begin(
        &self,
        _request: Nip44DecryptSessionBeginRequest,
    ) -> SignerOp<Nip44DecryptSessionGrant> {
        op_from_slots(&self.state.ready.begin, &self.state.pending.begin)
    }
    fn nip44_decrypt_batch(
        &self,
        _request: Nip44DecryptBatchRequest,
    ) -> SignerOp<Nip44DecryptBatchResult> {
        op_from_slots(&self.state.ready.batch, &self.state.pending.batch)
    }
    fn nip44_decrypt_session_end(&self, _request: Nip44DecryptSessionEndRequest) -> SignerOp<bool> {
        op_from_slots(&self.state.ready.end, &self.state.pending.end)
    }
    fn deliver_response(&self, _response_json: &str) {
        self.state.deliver_ok();
    }
}

fn op_from_slots<T: Clone + Send + 'static>(
    ready: &Mutex<Option<Result<T, SignerError>>>,
    pending: &Mutex<Option<Sender<Result<T, SignerError>>>>,
) -> SignerOp<T> {
    if let Some(result) = ready.lock().unwrap().clone() {
        return SignerOp::Ready(result);
    }
    let (tx, rx) = channel();
    *pending.lock().unwrap() = Some(tx);
    SignerOp::Pending(rx)
}

fn add_session_signer(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    state: Arc<SessionSignerState>,
) {
    commands::add_signer(
        identity,
        kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(SessionSigner { state })),
        true,
        false,
    );
}

fn success_batch() -> Nip44DecryptBatchResult {
    Nip44DecryptBatchResult {
        items: vec![
            Nip44DecryptBatchItemResult {
                id: "item-a".to_string(),
                plaintext: Some("plain-a".to_string()),
                error: None,
            },
            Nip44DecryptBatchItemResult {
                id: "item-b".to_string(),
                plaintext: None,
                error: Some("failed-b".to_string()),
            },
        ],
    }
}

#[test]
fn unsupported_begin_resolves_as_data_for_scalar_fallback() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let state = SessionSignerState::new();
    *state.ready.begin.lock().unwrap() = Some(Err(SignerError::Unsupported(
        "extension not negotiated".to_string(),
    )));
    add_session_signer(&mut identity, &mut kernel, Arc::clone(&state));

    let (captured, continuation) = capture_begin();
    let parked = dispatch_one(
        ActorCommand::Sign(SignCommand::Nip44DecryptSessionBegin {
            request: begin_request(),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );

    assert!(parked.is_empty());
    let outcome = captured.lock().unwrap().take().expect("continuation ran");
    assert!(matches!(
        outcome.expect("unsupported is data"),
        Nip44DecryptSessionBeginPortResult::Unsupported { .. }
    ));
}

#[test]
fn begin_completion_is_delivered_through_mailbox_and_parked_drain() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let state = SessionSignerState::new();
    add_session_signer(&mut identity, &mut kernel, Arc::clone(&state));

    let (captured, continuation) = capture_begin();
    let mut parked = dispatch_one(
        ActorCommand::Sign(SignCommand::Nip44DecryptSessionBegin {
            request: begin_request(),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );
    assert_eq!(parked.len(), 1);
    assert!(captured.lock().unwrap().is_none());

    let response_json = r#"{"id":"begin","result":"ok"}"#.to_string();
    let mailbox_parked = dispatch_one(
        ActorCommand::Identity(IdentityCommand::DeliverSignerResponse { response_json }),
        &mut identity,
        &mut kernel,
    );
    assert!(mailbox_parked.is_empty());

    let drained = resolve_parked_op(&mut parked[0], &mut kernel);
    assert!(!drained.keep);
    let outcome = captured.lock().unwrap().take().expect("continuation ran");
    assert!(matches!(
        outcome.expect("grant"),
        Nip44DecryptSessionBeginPortResult::Granted(_)
    ));
}

#[test]
fn batch_per_item_failure_is_typed_data() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let state = SessionSignerState::new();
    *state.ready.batch.lock().unwrap() = Some(Ok(success_batch()));
    add_session_signer(&mut identity, &mut kernel, Arc::clone(&state));

    let (captured, continuation) = capture_batch();
    let parked = dispatch_one(
        ActorCommand::Sign(SignCommand::Nip44DecryptBatch {
            request: batch_request(),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );

    assert!(parked.is_empty());
    let outcome = captured.lock().unwrap().take().expect("continuation ran");
    let Nip44DecryptBatchPortResult::Batch(batch) = outcome.expect("batch ok") else {
        panic!("expected batch outcome");
    };
    assert!(matches!(
        &batch.items[0],
        Nip44DecryptBatchItemPortOutcome::Plaintext { plaintext, .. } if plaintext == "plain-a"
    ));
    assert!(matches!(
        &batch.items[1],
        Nip44DecryptBatchItemPortOutcome::Failed { error, .. } if error == "failed-b"
    ));
}

#[test]
fn malformed_batch_item_shape_resolves_err() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let state = SessionSignerState::new();
    *state.ready.batch.lock().unwrap() = Some(Ok(Nip44DecryptBatchResult {
        items: vec![
            Nip44DecryptBatchItemResult {
                id: "item-a".to_string(),
                plaintext: Some("plain-a".to_string()),
                error: Some("also-error".to_string()),
            },
            Nip44DecryptBatchItemResult {
                id: "item-b".to_string(),
                plaintext: None,
                error: Some("failed-b".to_string()),
            },
        ],
    }));
    add_session_signer(&mut identity, &mut kernel, Arc::clone(&state));

    let (captured, continuation) = capture_batch();
    let parked = dispatch_one(
        ActorCommand::Sign(SignCommand::Nip44DecryptBatch {
            request: batch_request(),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );

    assert!(parked.is_empty());
    let err = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation ran")
        .expect_err("malformed batch is an actor error");
    assert!(err.contains("expected exactly one"), "got: {err}");
}

#[test]
fn batch_timeout_resolves_continuation_err() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let state = SessionSignerState::new();
    add_session_signer(&mut identity, &mut kernel, Arc::clone(&state));

    let (captured, continuation) = capture_batch();
    let mut parked = dispatch_one(
        ActorCommand::Sign(SignCommand::Nip44DecryptBatch {
            request: batch_request(),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );
    assert_eq!(parked.len(), 1);
    parked[0].deadline = crate::time::Instant::now() - Duration::from_millis(1);

    let drained = resolve_parked_op(&mut parked[0], &mut kernel);
    assert!(!drained.keep);
    let err = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation ran")
        .expect_err("timeout is an error terminal");
    assert_eq!(err, "nip44 decrypt batch timed out");
}

#[test]
fn session_end_acknowledgement_is_typed() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let state = SessionSignerState::new();
    *state.ready.end.lock().unwrap() = Some(Ok(true));
    add_session_signer(&mut identity, &mut kernel, Arc::clone(&state));

    let (captured, continuation) = capture_end();
    let parked = dispatch_one(
        ActorCommand::Sign(SignCommand::Nip44DecryptSessionEnd {
            request: end_request(),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );

    assert!(parked.is_empty());
    let outcome = captured.lock().unwrap().take().expect("continuation ran");
    assert_eq!(
        outcome.expect("end ok"),
        Nip44DecryptSessionEndPortResult::Ended { acknowledged: true }
    );
}

#[test]
fn debug_output_redacts_secret_bearing_session_values() {
    let batch = Nip44DecryptBatchPortResult::Batch(Nip44DecryptBatchPortOutcome {
        items: vec![Nip44DecryptBatchItemPortOutcome::Plaintext {
            id: "item-a".to_string(),
            plaintext: "plain-a".to_string(),
        }],
    });
    let rendered = format!("{batch:?} {:?}", batch_request());
    assert!(!rendered.contains("plain-a"));
    assert!(!rendered.contains("cipher-a"));
    assert!(!rendered.contains("session-secret"));
}
