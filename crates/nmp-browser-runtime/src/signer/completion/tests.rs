use std::sync::{mpsc, Arc};

use nmp_signer_iface::{Nip46Rpc, Nip46Transport};
use nmp_signers::{LocalKeySigner, Nip46SignerHandle, Signer};

use super::*;
use crate::signer::registry::CapabilityProviderRegistry;

/// A no-op wake cell for broker tests that don't assert on wake firing.
fn noop_wake() -> WakeCell {
    use std::cell::RefCell;
    use std::rc::Rc;
    Rc::new(RefCell::new(Rc::new(|| {}) as Rc<dyn Fn()>))
}

fn make_registry_with_local_key(secret_hex: &str) -> (CapabilityProviderRegistry, String) {
    let signer = LocalKeySigner::from_secret_hex(secret_hex).expect("valid secret");
    let pubkey_hex = signer.pubkey().to_hex();
    let mut reg = CapabilityProviderRegistry::new();
    reg.insert(Arc::new(signer) as Arc<dyn Signer>);
    (reg, pubkey_hex)
}

/// A minimal unsigned event JSON in the flat wire shape.
fn unsigned_json(pubkey: &str) -> String {
    serde_json::json!({
        "pubkey": pubkey,
        "kind": 1,
        "tags": [],
        "content": "test",
        "created_at": 1_700_000_000u64,
    })
    .to_string()
}

#[test]
fn local_key_broker_sends_completion() {
    let secret = "bb".repeat(32);
    let (reg, pubkey_hex) = make_registry_with_local_key(&secret);
    let (tx, rx) = mpsc::channel::<SignerCompletion>();
    let ujson = unsigned_json(&pubkey_hex);
    let mut pending = PendingSignerCompletions::new();

    let brokered = broker_sign_request(
        &reg,
        &mut pending,
        "corr-1",
        &pubkey_hex,
        &ujson,
        &tx,
        &noop_wake(),
    );

    assert!(brokered, "LocalKey should be brokered");
    let completion = rx.try_recv().expect("completion must arrive synchronously");
    assert_eq!(completion.correlation_id, "corr-1");
    assert!(
        completion.result.is_ok(),
        "LocalKey sign must succeed: {:?}",
        completion.result
    );
}

#[test]
fn unknown_pubkey_returns_false() {
    let (reg, _) = make_registry_with_local_key(&"cc".repeat(32));
    let (tx, _rx) = mpsc::channel::<SignerCompletion>();
    let mut pending = PendingSignerCompletions::new();

    let brokered = broker_sign_request(
        &reg,
        &mut pending,
        "corr-2",
        "deadbeef",
        "{}",
        &tx,
        &noop_wake(),
    );
    assert!(!brokered, "unknown pubkey must not be brokered");
}

#[test]
fn enqueue_completion_fires_wake_and_queues() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    let count = Rc::new(Cell::new(0u32));
    let count_clone = Rc::clone(&count);
    let wake: WakeCell = Rc::new(RefCell::new(Rc::new(move || {
        count_clone.set(count_clone.get() + 1);
    }) as Rc<dyn Fn()>));

    let (tx, rx) = mpsc::channel::<SignerCompletion>();
    enqueue_completion(
        &tx,
        &wake,
        SignerCompletion {
            correlation_id: "corr-wake".to_string(),
            result: Ok("{}".to_string()),
        },
    );

    assert_eq!(count.get(), 1, "enqueue_completion must fire the wake once");
    let completion = rx.try_recv().expect("completion must be queued");
    assert_eq!(completion.correlation_id, "corr-wake");
}

#[test]
fn malformed_unsigned_json_sends_error_completion() {
    let secret = "dd".repeat(32);
    let (reg, pubkey_hex) = make_registry_with_local_key(&secret);
    let (tx, rx) = mpsc::channel::<SignerCompletion>();
    let mut pending = PendingSignerCompletions::new();

    let brokered = broker_sign_request(
        &reg,
        &mut pending,
        "corr-3",
        &pubkey_hex,
        "not-valid-json",
        &tx,
        &noop_wake(),
    );
    assert!(
        brokered,
        "malformed json still triggers broker (error path)"
    );
    let completion = rx.try_recv().expect("error completion must arrive");
    assert!(
        completion.result.is_err(),
        "malformed JSON must produce error completion"
    );
}

#[derive(Debug, Default)]
struct StubTransport {
    sent: std::sync::Mutex<Vec<Nip46Rpc>>,
}

impl Nip46Transport for StubTransport {
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        self.sent.lock().expect("sent lock").push(rpc);
        Ok(())
    }
}

#[derive(Debug)]
struct FailingTransport;

impl Nip46Transport for FailingTransport {
    fn send_rpc(&self, _rpc: Nip46Rpc) -> Result<(), SignerError> {
        Err(SignerError::Backend("transport closed".to_string()))
    }
}

fn nip46_signer_with_transport<T: Nip46Transport + 'static>(
    remote_user: &LocalKeySigner,
    transport: Arc<T>,
) -> nmp_signers::Nip46Signer {
    let uri = format!(
        "bunker://{}?relay=wss://relay.example.com",
        remote_user.pubkey().to_hex()
    );
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("valid bunker uri");
    handle.complete(transport, remote_user.pubkey())
}

#[test]
fn nip46_broker_parks_rpc_and_drains_after_response() {
    let remote_user = LocalKeySigner::generate();
    let pubkey_hex = remote_user.pubkey().to_hex();
    let transport = Arc::new(StubTransport::default());
    let signer = Arc::new(nip46_signer_with_transport(
        &remote_user,
        Arc::clone(&transport),
    ));
    let mut reg = CapabilityProviderRegistry::new();
    reg.insert_nip46(Arc::clone(&signer));
    let (tx, rx) = mpsc::channel::<SignerCompletion>();
    let mut pending = PendingSignerCompletions::new();
    let ujson = unsigned_json(&pubkey_hex);

    let brokered = broker_sign_request(
        &reg,
        &mut pending,
        "corr-nip46",
        &pubkey_hex,
        &ujson,
        &tx,
        &noop_wake(),
    );

    assert!(brokered, "NIP-46 signer should be brokered");
    assert!(
        rx.try_recv().is_err(),
        "pending NIP-46 sign must not complete before relay response"
    );
    let sent = transport.sent.lock().expect("sent lock").clone();
    assert_eq!(sent.len(), 1, "one NIP-46 sign_event RPC is queued");

    let unsigned = parse_unsigned_json(&ujson).expect("unsigned parses");
    let signed = match remote_user.sign(unsigned) {
        SignerOp::Ready(Ok(signed)) => signed,
        other => panic!("local fixture sign must complete: {other:?}"),
    };
    let response = serde_json::json!({
        "id": sent[0].id,
        "result": signed.to_nip01_json(),
    })
    .to_string();
    signer.ingest_rpc_response(&response);

    let ready = pending.drain_ready();
    assert_eq!(ready.len(), 1, "response must settle one pending sign");
    assert_eq!(ready[0].correlation_id, "corr-nip46");
    assert!(
        ready[0]
            .result
            .as_ref()
            .is_ok_and(|json| json.contains(&signed.id)),
        "completion must carry signed JSON: {:?}",
        ready[0].result
    );
}

#[test]
fn nip46_transport_error_sends_error_completion() {
    let remote_user = LocalKeySigner::generate();
    let pubkey_hex = remote_user.pubkey().to_hex();
    let signer = Arc::new(nip46_signer_with_transport(
        &remote_user,
        Arc::new(FailingTransport),
    ));
    let mut reg = CapabilityProviderRegistry::new();
    reg.insert_nip46(signer);
    let (tx, rx) = mpsc::channel::<SignerCompletion>();
    let mut pending = PendingSignerCompletions::new();

    let brokered = broker_sign_request(
        &reg,
        &mut pending,
        "corr-nip46-fail",
        &pubkey_hex,
        &unsigned_json(&pubkey_hex),
        &tx,
        &noop_wake(),
    );

    assert!(
        brokered,
        "NIP-46 transport failure still resolves broker path"
    );
    let completion = rx.try_recv().expect("error completion is synchronous");
    assert_eq!(completion.correlation_id, "corr-nip46-fail");
    assert!(completion
        .result
        .expect_err("failure must be reported")
        .contains("transport closed"));
    assert!(
        pending.drain_ready().is_empty(),
        "failed send must not leave a parked op"
    );
}
