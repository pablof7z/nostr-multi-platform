use std::sync::{Arc, Mutex};

use nmp_core::actor::CipherContinuation;
use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};
use nmp_signers::{LocalKeySigner, Nip46SignerHandle, Signer};

use super::*;
use crate::signer::registry::CapabilityProviderRegistry;

type CapturedCipher = Arc<Mutex<Option<Result<String, String>>>>;

fn capture_cipher() -> (CapturedCipher, CipherContinuation) {
    let captured = Arc::new(Mutex::new(None));
    let captured_for_continuation = Arc::clone(&captured);
    let continuation = CipherContinuation::new(move |outcome| {
        *captured_for_continuation.lock().expect("capture lock") = Some(outcome);
    });
    (captured, continuation)
}

#[test]
fn local_key_encrypt_resolves_inline() {
    let signer = LocalKeySigner::from_secret_hex(&"31".repeat(32)).expect("valid secret");
    let account_pubkey = signer.pubkey().to_hex();
    let peer = LocalKeySigner::from_secret_hex(&"32".repeat(32)).expect("valid secret");
    let mut registry = CapabilityProviderRegistry::new();
    registry.insert(Arc::new(signer) as Arc<dyn Signer>);
    let mut pending = PendingCipherCompletions::new();
    let (captured, continuation) = capture_cipher();

    dispatch_nip44_cipher(
        &registry,
        &mut pending,
        &account_pubkey,
        &peer.pubkey().to_hex(),
        "secret",
        Nip44CipherMode::Encrypt,
        continuation,
    );

    assert_eq!(pending.len(), 0, "local key must not park cipher ops");
    assert!(
        captured
            .lock()
            .expect("capture lock")
            .as_ref()
            .expect("continuation called")
            .is_ok(),
        "local key encrypt must resolve successfully"
    );
}

#[derive(Debug, Default)]
struct StubTransport {
    sent: Mutex<Vec<Nip46Rpc>>,
}

impl Nip46Transport for StubTransport {
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        self.sent.lock().expect("sent lock").push(rpc);
        Ok(())
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
fn nip46_encrypt_parks_then_resolves_after_rpc_response() {
    let remote_user = LocalKeySigner::generate();
    let account_pubkey = remote_user.pubkey().to_hex();
    let peer = LocalKeySigner::generate();
    let transport = Arc::new(StubTransport::default());
    let signer = Arc::new(nip46_signer_with_transport(
        &remote_user,
        Arc::clone(&transport),
    ));
    let mut registry = CapabilityProviderRegistry::new();
    registry.insert_nip46(Arc::clone(&signer));
    let mut pending = PendingCipherCompletions::new();
    let (captured, continuation) = capture_cipher();

    dispatch_nip44_cipher(
        &registry,
        &mut pending,
        &account_pubkey,
        &peer.pubkey().to_hex(),
        "seal plaintext",
        Nip44CipherMode::Encrypt,
        continuation,
    );

    assert_eq!(pending.len(), 1, "NIP-46 cipher op must park");
    assert!(
        captured.lock().expect("capture lock").is_none(),
        "continuation must wait for RPC response"
    );
    let sent = transport.sent.lock().expect("sent lock").clone();
    assert_eq!(sent.len(), 1, "one NIP-46 cipher RPC is queued");
    assert!(sent[0].body_json.contains(r#""method":"nip44_encrypt""#));

    let response = serde_json::json!({
        "id": sent[0].id,
        "result": "ciphertext-from-bunker",
    })
    .to_string();
    signer.ingest_rpc_response(&response);

    pending.drain_ready();
    assert_eq!(pending.len(), 0, "ready op must be removed");
    assert_eq!(
        captured
            .lock()
            .expect("capture lock")
            .take()
            .expect("continuation called")
            .expect("cipher result succeeds"),
        "ciphertext-from-bunker"
    );
}

#[test]
fn nip46_decrypt_uses_decrypt_method() {
    let remote_user = LocalKeySigner::generate();
    let account_pubkey = remote_user.pubkey().to_hex();
    let peer = LocalKeySigner::generate();
    let transport = Arc::new(StubTransport::default());
    let signer = Arc::new(nip46_signer_with_transport(
        &remote_user,
        Arc::clone(&transport),
    ));
    let mut registry = CapabilityProviderRegistry::new();
    registry.insert_nip46(signer);
    let mut pending = PendingCipherCompletions::new();
    let (_captured, continuation) = capture_cipher();

    dispatch_nip44_cipher(
        &registry,
        &mut pending,
        &account_pubkey,
        &peer.pubkey().to_hex(),
        "cipher-payload",
        Nip44CipherMode::Decrypt,
        continuation,
    );

    let sent = transport.sent.lock().expect("sent lock").clone();
    assert_eq!(sent.len(), 1, "one NIP-46 decrypt RPC is queued");
    assert!(sent[0].body_json.contains(r#""method":"nip44_decrypt""#));
}

#[test]
fn nip46_error_response_resolves_continuation_as_failure() {
    let remote_user = LocalKeySigner::generate();
    let account_pubkey = remote_user.pubkey().to_hex();
    let peer = LocalKeySigner::generate();
    let transport = Arc::new(StubTransport::default());
    let signer = Arc::new(nip46_signer_with_transport(
        &remote_user,
        Arc::clone(&transport),
    ));
    let mut registry = CapabilityProviderRegistry::new();
    registry.insert_nip46(Arc::clone(&signer));
    let mut pending = PendingCipherCompletions::new();
    let (captured, continuation) = capture_cipher();

    dispatch_nip44_cipher(
        &registry,
        &mut pending,
        &account_pubkey,
        &peer.pubkey().to_hex(),
        "seal plaintext",
        Nip44CipherMode::Encrypt,
        continuation,
    );

    let sent = transport.sent.lock().expect("sent lock").clone();
    let response = serde_json::json!({
        "id": sent[0].id,
        "error": "user rejected",
    })
    .to_string();
    signer.ingest_rpc_response(&response);

    pending.drain_ready();
    assert_eq!(pending.len(), 0, "terminal error must be removed");
    let error = captured
        .lock()
        .expect("capture lock")
        .take()
        .expect("continuation called")
        .expect_err("cipher result fails");
    assert!(
        error.contains("user rejected"),
        "provider error must surface through continuation; got {error}"
    );
}
