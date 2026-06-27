use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_signer_iface::{
    Nip44DecryptBatchItem, Nip44DecryptBatchItemResult, Nip44DecryptBatchRequest,
    Nip44DecryptBatchResult, Nip44DecryptSessionBeginRequest, Nip44DecryptSessionEndRequest,
    Nip46Rpc, Nip46Transport, RemoteSignerHandle, SignerError, NMP_NIP44_BACKFILL_SCOPE,
    NMP_NIP44_DECRYPT_BATCH, NMP_NIP44_DECRYPT_SESSION_BEGIN, NMP_NIP44_DECRYPT_SESSION_END,
};

use crate::signers::payload::SignerPayload;
use crate::signers::traits::Signer;
use crate::{LocalKeySigner, Nip46Signer, Nip46SignerHandle};

const SAMPLE_PK: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[derive(Debug, Default)]
struct StubTransport {
    sent: Mutex<Vec<Nip46Rpc>>,
}

impl Nip46Transport for StubTransport {
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        self.sent.lock().unwrap().push(rpc);
        Ok(())
    }
}

fn build_signer_with_remote(remote_user: &LocalKeySigner) -> (Nip46Signer, Arc<StubTransport>) {
    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com&secret=s1");
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
    let transport = Arc::new(StubTransport::default());
    let signer = handle.complete(transport.clone(), remote_user.pubkey());
    (signer, transport)
}

#[test]
fn begin_sends_extension_rpc_and_persists_support_on_success() {
    let remote_user = LocalKeySigner::generate();
    let (signer, transport) = build_signer_with_remote(&remote_user);

    let op = signer.nip44_decrypt_session_begin(Nip44DecryptSessionBeginRequest {
        scope: NMP_NIP44_BACKFILL_SCOPE.to_string(),
        requester_pubkey: remote_user.pubkey().to_hex(),
        max_items: 512,
        expires_at: 1_800_000_000,
    });

    let sent = transport.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].body_json_to_encrypt, sent[0].body_json);
    assert!(sent[0].body_json.contains(NMP_NIP44_DECRYPT_SESSION_BEGIN));
    assert!(sent[0]
        .body_json
        .contains(r#""scope":"nmp.nip44.backfill""#));
    let rpc_id = sent[0].id.clone();

    signer.deliver_response(
        &serde_json::json!({
            "id": rpc_id,
            "result": {
                "session_id": "opaque-session-token",
                "max_batch_items": 128,
                "expires_at": 1_800_000_000_u64,
            },
        })
        .to_string(),
    );

    let grant = op
        .wait(Duration::from_secs(2))
        .expect("begin result must parse");
    assert_eq!(grant.session_id, "opaque-session-token");
    assert_eq!(grant.max_batch_items, 128);

    let SignerPayload::Nip46(payload) = signer.to_payload().expect("to_payload") else {
        panic!("expected nip46 payload");
    };
    assert_eq!(
        payload
            .nip44_decrypt_session_extension
            .expect("extension metadata")
            .version,
        1
    );
}

#[test]
fn batch_maps_object_result_and_per_item_errors() {
    let remote_user = LocalKeySigner::generate();
    let (signer, transport) = build_signer_with_remote(&remote_user);

    let op = signer.nip44_decrypt_batch(Nip44DecryptBatchRequest {
        session_id: "opaque-session-token".to_string(),
        items: vec![Nip44DecryptBatchItem {
            id: "outer:1".to_string(),
            peer_pubkey: "44".repeat(32),
            ciphertext: "ciphertext".to_string(),
        }],
    });

    let sent = transport.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].body_json.contains(NMP_NIP44_DECRYPT_BATCH));
    assert!(sent[0].body_json.contains(r#""items":[{"id":"outer:1""#));
    let rpc_id = sent[0].id.clone();

    signer.deliver_response(
        &serde_json::json!({
            "id": rpc_id,
            "result": {
                "items": [
                    {"id": "outer:1", "plaintext": "seal-json"},
                    {"id": "outer:bad", "error": "decrypt_failed"},
                ],
            },
        })
        .to_string(),
    );

    let result = op.wait(Duration::from_secs(2)).expect("batch result");
    assert_eq!(
        result,
        Nip44DecryptBatchResult {
            items: vec![
                Nip44DecryptBatchItemResult {
                    id: "outer:1".to_string(),
                    plaintext: Some("seal-json".to_string()),
                    error: None,
                },
                Nip44DecryptBatchItemResult {
                    id: "outer:bad".to_string(),
                    plaintext: None,
                    error: Some("decrypt_failed".to_string()),
                },
            ],
        }
    );
}

#[test]
fn end_maps_boolean_result() {
    let remote_user = LocalKeySigner::generate();
    let (signer, transport) = build_signer_with_remote(&remote_user);

    let op = signer.nip44_decrypt_session_end(Nip44DecryptSessionEndRequest {
        session_id: "opaque-session-token".to_string(),
    });

    let sent = transport.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].body_json.contains(NMP_NIP44_DECRYPT_SESSION_END));
    let rpc_id = sent[0].id.clone();

    signer.deliver_response(
        &serde_json::json!({
            "id": rpc_id,
            "result": true,
        })
        .to_string(),
    );

    assert!(op
        .wait(Duration::from_secs(2))
        .expect("boolean result must parse"));
}

#[test]
fn malformed_batch_response_surfaces_backend_error() {
    let remote_user = LocalKeySigner::generate();
    let (signer, transport) = build_signer_with_remote(&remote_user);

    let op = signer.nip44_decrypt_batch(Nip44DecryptBatchRequest {
        session_id: "opaque-session-token".to_string(),
        items: Vec::new(),
    });
    let rpc_id = transport.sent.lock().unwrap()[0].id.clone();

    signer.deliver_response(
        &serde_json::json!({
            "id": rpc_id,
            "result": {"items": "not-an-array"},
        })
        .to_string(),
    );

    let err = op
        .wait(Duration::from_secs(2))
        .expect_err("malformed batch response must fail");
    assert!(matches!(err, SignerError::Backend(m) if m.contains("malformed")));
}

#[test]
fn restored_payload_preserves_extension_metadata() {
    let remote_user = LocalKeySigner::generate();
    let (signer, _transport) = build_signer_with_remote(&remote_user);
    let SignerPayload::Nip46(mut payload) = signer.to_payload().expect("to_payload") else {
        panic!("expected nip46 payload");
    };
    payload.nip44_decrypt_session_extension =
        Some(nmp_signer_iface::Nip44DecryptSessionExtension::default());

    let restored =
        Nip46Signer::from_payload(&payload, Arc::new(StubTransport::default())).expect("restore");
    let SignerPayload::Nip46(restored_payload) = restored.to_payload().expect("to_payload") else {
        panic!("expected nip46 payload");
    };
    assert_eq!(
        restored_payload.nip44_decrypt_session_extension,
        payload.nip44_decrypt_session_extension
    );
}
