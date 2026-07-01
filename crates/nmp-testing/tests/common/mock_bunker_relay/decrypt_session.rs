use nmp_signer_iface::{
    NMP_NIP44_BACKFILL_SCOPE, NMP_NIP44_DECRYPT_BATCH, NMP_NIP44_DECRYPT_SESSION_BEGIN,
};
use nostr::nips::nip44;
use nostr::{Keys, PublicKey};
use serde_json::{json, Value};

const SESSION_ID: &str = "mock-bunker-decrypt-session";

pub(super) fn begin_result(params: &Value) -> Option<Value> {
    let request = first_param(params)?;
    if request.get("scope")?.as_str()? != NMP_NIP44_BACKFILL_SCOPE {
        return Some(error_result(
            "unsupported_scope",
            NMP_NIP44_DECRYPT_SESSION_BEGIN,
        ));
    }
    Some(json!({
        "session_id": SESSION_ID,
        "max_batch_items": 128,
        "expires_at": request.get("expires_at").and_then(Value::as_u64).unwrap_or(0),
    }))
}

pub(super) fn batch_result(params: &Value, user_keys: &Keys) -> Option<Value> {
    let request = first_param(params)?;
    if request.get("session_id")?.as_str()? != SESSION_ID {
        return Some(error_result("bad_session", NMP_NIP44_DECRYPT_BATCH));
    }

    let mut results = Vec::new();
    for item in request.get("items")?.as_array()? {
        let id = item.get("id")?.as_str()?.to_string();
        let peer = item
            .get("peer_pubkey")
            .and_then(Value::as_str)
            .and_then(|hex| PublicKey::from_hex(hex).ok());
        let ciphertext = item.get("ciphertext").and_then(Value::as_str);
        match (peer, ciphertext) {
            (Some(peer), Some(ciphertext)) => {
                match nip44::decrypt(user_keys.secret_key(), &peer, ciphertext) {
                    Ok(plaintext) => results.push(json!({ "id": id, "plaintext": plaintext })),
                    Err(_) => results.push(json!({ "id": id, "error": "decrypt_failed" })),
                }
            }
            _ => results.push(json!({ "id": id, "error": "malformed_item" })),
        }
    }

    Some(json!({ "items": results }))
}

pub(super) fn end_result(params: &Value) -> Option<Value> {
    let request = first_param(params)?;
    Some(json!(request.get("session_id")?.as_str()? == SESSION_ID))
}

fn first_param(params: &Value) -> Option<&Value> {
    params.as_array()?.first()
}

fn error_result(code: &str, method: &str) -> Value {
    json!({
        "items": [{
            "id": method,
            "error": code,
        }],
    })
}
