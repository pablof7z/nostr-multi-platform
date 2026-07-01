//! Contract tests for the optional NMP NIP-44 decrypt-session extension.

use std::time::Duration;

use nmp_signer_iface::{
    Nip44DecryptBatchItem, Nip44DecryptBatchItemResult, Nip44DecryptBatchRequest,
    Nip44DecryptBatchResult, Nip44DecryptSessionBeginRequest, Nip44DecryptSessionEndRequest,
    Nip44DecryptSessionExtension, Nip44DecryptSessionGrant, RemoteSignerHandle, SignedEvent,
    SignerError, SignerOp, UnsignedEvent, NMP_NIP44_BACKFILL_SCOPE,
    NMP_NIP44_DECRYPT_SESSION_EXTENSION_VERSION,
};

#[derive(Debug)]
struct UnsupportedSigner;

impl RemoteSignerHandle for UnsupportedSigner {
    fn pubkey_hex(&self) -> String {
        "00".repeat(32)
    }

    fn signer_kind(&self) -> &'static str {
        "unsupported-test"
    }

    fn sign(&self, _unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        SignerOp::err(SignerError::Unsupported("sign".to_string()))
    }

    fn nip44_encrypt(&self, _recipient_pubkey: &str, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported("encrypt".to_string()))
    }

    fn nip44_decrypt(&self, _sender_pubkey: &str, _ciphertext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Unsupported("decrypt".to_string()))
    }

    fn deliver_response(&self, _response_json: &str) {}
}

#[test]
fn non_capable_signer_defaults_to_unsupported_session_methods() {
    let signer = UnsupportedSigner;

    let begin = signer.nip44_decrypt_session_begin(Nip44DecryptSessionBeginRequest {
        scope: NMP_NIP44_BACKFILL_SCOPE.to_string(),
        requester_pubkey: "11".repeat(32),
        max_items: 512,
        expires_at: 1_800_000_000,
    });
    assert!(matches!(
        begin.wait(Duration::from_millis(1)),
        Err(SignerError::Unsupported(_))
    ));

    let batch = signer.nip44_decrypt_batch(Nip44DecryptBatchRequest {
        session_id: "session-secret".to_string(),
        items: Vec::new(),
    });
    assert!(matches!(
        batch.wait(Duration::from_millis(1)),
        Err(SignerError::Unsupported(_))
    ));

    let end = signer.nip44_decrypt_session_end(Nip44DecryptSessionEndRequest {
        session_id: "session-secret".to_string(),
    });
    assert!(matches!(
        end.wait(Duration::from_millis(1)),
        Err(SignerError::Unsupported(_))
    ));
}

#[test]
fn begin_request_and_grant_match_wire_shape() {
    let request = Nip44DecryptSessionBeginRequest {
        scope: NMP_NIP44_BACKFILL_SCOPE.to_string(),
        requester_pubkey: "22".repeat(32),
        max_items: 512,
        expires_at: 1_800_000_000,
    };
    let json = serde_json::to_string(&request).expect("serialize request");
    assert!(json.contains(r#""scope":"nmp.nip44.backfill""#));
    assert!(json.contains(r#""requester_pubkey":"#));
    assert!(json.contains(r#""max_items":512"#));
    assert!(json.contains(r#""expires_at":1800000000"#));

    let grant: Nip44DecryptSessionGrant = serde_json::from_str(
        r#"{"session_id":"opaque-token","max_batch_items":128,"expires_at":1800000000}"#,
    )
    .expect("parse grant");
    assert_eq!(grant.session_id, "opaque-token");
    assert_eq!(grant.max_batch_items, 128);
}

#[test]
fn batch_result_allows_plaintext_or_per_item_error() {
    let parsed: Nip44DecryptBatchResult = serde_json::from_str(
        r#"{"items":[{"id":"outer:ok","plaintext":"rumor-json"},{"id":"outer:bad","error":"decrypt_failed"}]}"#,
    )
    .expect("parse batch result");
    assert_eq!(
        parsed.items,
        vec![
            Nip44DecryptBatchItemResult {
                id: "outer:ok".to_string(),
                plaintext: Some("rumor-json".to_string()),
                error: None,
            },
            Nip44DecryptBatchItemResult {
                id: "outer:bad".to_string(),
                plaintext: None,
                error: Some("decrypt_failed".to_string()),
            },
        ]
    );
}

#[test]
fn debug_redacts_secret_bearing_fields() {
    let grant = Nip44DecryptSessionGrant {
        session_id: "session-secret".to_string(),
        max_batch_items: 128,
        expires_at: 1_800_000_000,
    };
    let s = format!("{grant:?}");
    assert!(!s.contains("session-secret"));

    let request = Nip44DecryptBatchRequest {
        session_id: "session-secret".to_string(),
        items: vec![Nip44DecryptBatchItem {
            id: "outer:1".to_string(),
            peer_pubkey: "33".repeat(32),
            ciphertext: "cipher-secret".to_string(),
        }],
    };
    let s = format!("{request:?}");
    assert!(!s.contains("session-secret"));
    assert!(!s.contains("cipher-secret"));

    let result = Nip44DecryptBatchResult {
        items: vec![Nip44DecryptBatchItemResult {
            id: "outer:1".to_string(),
            plaintext: Some("plain-secret".to_string()),
            error: None,
        }],
    };
    let s = format!("{result:?}");
    assert!(!s.contains("plain-secret"));

    let end = Nip44DecryptSessionEndRequest {
        session_id: "session-secret".to_string(),
    };
    let s = format!("{end:?}");
    assert!(!s.contains("session-secret"));
}

#[test]
fn extension_metadata_defaults_to_current_version() {
    let extension = Nip44DecryptSessionExtension::default();
    assert_eq!(
        extension.version,
        NMP_NIP44_DECRYPT_SESSION_EXTENSION_VERSION
    );
}
