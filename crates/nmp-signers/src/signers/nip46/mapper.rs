//! Response mapping helpers for `Nip46Signer`.
//!
//! Extracted from the main module to keep `mod.rs` under the 300 LOC soft cap.
//!
//! `map_response_to_event` adapts the inner `Result<String, SignerError>`
//! stream into a `Result<SignedEvent, SignerError>` stream.  For pending ops
//! it spawns a dedicated worker thread — the actor loop is sync mpsc and
//! does not have an executor; this is the cheapest way to pipeline async
//! transforms without pulling in Tokio.

use std::sync::mpsc;

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::PublicKey;

use crate::signers::{SignerError, SignerOp};

/// Map the raw NIP-46 RPC response (`String`) into a `SignedEvent` thunk.
pub fn map_response_to_event(
    raw_op: SignerOp<String>,
    unsigned: UnsignedEvent,
    expected_pubkey: PublicKey,
) -> SignerOp<SignedEvent> {
    match raw_op {
        SignerOp::Ready(Ok(s)) => SignerOp::Ready(parse_signed_event_response(
            &s,
            &unsigned,
            expected_pubkey,
        )),
        SignerOp::Ready(Err(e)) => SignerOp::Ready(Err(e)),
        SignerOp::Pending(rx) => {
            let (tx, out_rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = match rx.recv() {
                    Ok(Ok(s)) => parse_signed_event_response(&s, &unsigned, expected_pubkey),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(SignerError::Backend(
                        "nip46 response channel disconnected".to_string(),
                    )),
                };
                let _ = tx.send(result);
            });
            SignerOp::Pending(out_rx)
        }
    }
}

/// Parse the JSON response body from a `sign_event` RPC.
pub fn parse_signed_event_response(
    s: &str,
    unsigned: &UnsignedEvent,
    expected_pubkey: PublicKey,
) -> Result<SignedEvent, SignerError> {
    let v: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| SignerError::Backend(format!("nip46 response not JSON: {e}")))?;
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| SignerError::Backend("missing id".to_string()))?
        .to_string();
    let sig = v
        .get("sig")
        .and_then(|x| x.as_str())
        .ok_or_else(|| SignerError::Backend("missing sig".to_string()))?
        .to_string();
    let pubkey_hex = v
        .get("pubkey")
        .and_then(|x| x.as_str())
        .ok_or_else(|| SignerError::Backend("missing pubkey".to_string()))?;
    if pubkey_hex != expected_pubkey.to_hex() {
        return Err(SignerError::Mismatch(format!(
            "signed event pubkey {pubkey_hex} != expected {}",
            expected_pubkey.to_hex()
        )));
    }
    Ok(SignedEvent {
        id,
        sig,
        unsigned: unsigned.clone(),
    })
}

/// Minimal JSON-string escape — covers what NIP-46 RPC params need.
pub fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Deterministic-ish request id (timestamp + atomic counter).  Not a security
/// boundary — uniqueness within the signer's lifetime is what matters.
pub fn generate_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{now:016x}{n:08x}")
}
