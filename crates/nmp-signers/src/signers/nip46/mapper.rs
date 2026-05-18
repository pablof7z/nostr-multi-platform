//! Response mapping helpers for `Nip46Signer`.
//!
//! Extracted from the main module to keep `mod.rs` under the 300 LOC soft cap.
//!
//! `map_response_to_event` adapts the inner `Result<String, SignerError>`
//! stream into a `Result<SignedEvent, SignerError>` stream.  For pending ops
//! it spawns a dedicated worker thread — the actor loop is sync mpsc and
//! does not have an executor; this is the cheapest way to pipeline async
//! transforms without pulling in Tokio.
//!
//! ## Trust model (codex review #3 — 9944bed.md)
//!
//! NIP-46 RPC responses **must not be trusted verbatim**.  A compromised
//! bunker (or a misbehaving one) could otherwise return a `{id, sig}` pair
//! pointing at an event the local kernel never asked to sign.  Every signed
//! event returned by `sign_event` is:
//!
//! 1. Reconstructed from the **response's own fields** (`pubkey`, `kind`,
//!    `tags`, `content`, `created_at`) — never from the local template, since
//!    the remote may have legitimately mutated `created_at` (clock skew) or
//!    re-ordered tags.
//! 2. Validated via `nostr::Event::verify()` — recomputes the event id from
//!    the canonical fields **and** checks the schnorr signature.
//! 3. Cross-checked: the response's claimed `pubkey` must equal the expected
//!    remote-user pubkey we cached at handshake time.
//!
//! Any failure surfaces as `SignerError::SignatureVerificationFailed` (sig /
//! id mismatch) or `SignerError::Mismatch` (pubkey mismatch).

use std::str::FromStr;
use std::sync::mpsc;

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::secp256k1::schnorr::Signature;
use nostr::{Event, EventId, Kind, PublicKey, Tag, Timestamp};

use crate::signers::{SignerError, SignerOp};

/// Map the raw NIP-46 RPC response (`String`) into a `SignedEvent` thunk.
///
/// `_unsigned` is kept in the signature for source-code locality (the caller
/// already has it) but is intentionally unused: per codex review #3, the
/// canonical event we trust is the one the remote returned, not the local
/// template.  The template's role ended when we sent the RPC.
pub fn map_response_to_event(
    raw_op: SignerOp<String>,
    _unsigned: UnsignedEvent,
    expected_pubkey: PublicKey,
) -> SignerOp<SignedEvent> {
    match raw_op {
        SignerOp::Ready(Ok(s)) => {
            SignerOp::Ready(parse_signed_event_response(&s, expected_pubkey))
        }
        SignerOp::Ready(Err(e)) => SignerOp::Ready(Err(e)),
        SignerOp::Pending(rx) => {
            let (tx, out_rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = match rx.recv() {
                    Ok(Ok(s)) => parse_signed_event_response(&s, expected_pubkey),
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

/// Parse the JSON response body from a `sign_event` RPC and verify it end to
/// end (id recomputation + schnorr signature, plus pubkey identity check).
pub fn parse_signed_event_response(
    s: &str,
    expected_pubkey: PublicKey,
) -> Result<SignedEvent, SignerError> {
    let v: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| SignerError::Backend(format!("nip46 response not JSON: {e}")))?;

    let pubkey_hex = required_str(&v, "pubkey")?;
    if pubkey_hex != expected_pubkey.to_hex() {
        return Err(SignerError::Mismatch(format!(
            "signed event pubkey {pubkey_hex} != expected {}",
            expected_pubkey.to_hex()
        )));
    }
    let kind_u64 = v
        .get("kind")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| SignerError::Backend("missing kind".to_string()))?;
    let created_at_u64 = v
        .get("created_at")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| SignerError::Backend("missing created_at".to_string()))?;
    let content = required_str(&v, "content")?.to_string();
    let id_hex = required_str(&v, "id")?.to_string();
    let sig_hex = required_str(&v, "sig")?.to_string();
    let tag_rows = parse_tag_rows(&v)?;

    // ----- Reconstruct the canonical event from the response's own fields
    // and run verify() — this checks BOTH the id (matches the canonical hash
    // of pubkey/created_at/kind/tags/content) AND the schnorr signature.
    let event = build_event_for_verify(
        &id_hex,
        expected_pubkey,
        created_at_u64,
        kind_u64,
        &tag_rows,
        &content,
        &sig_hex,
    )?;
    event.verify().map_err(|e| {
        SignerError::SignatureVerificationFailed(format!(
            "nip46 sign_event response failed verify(): {e}"
        ))
    })?;

    Ok(SignedEvent {
        id: id_hex,
        sig: sig_hex,
        unsigned: UnsignedEvent {
            pubkey: expected_pubkey.to_hex(),
            kind: u32::try_from(kind_u64).map_err(|_| {
                SignerError::Backend(format!("kind {kind_u64} does not fit in u32"))
            })?,
            tags: tag_rows,
            content,
            created_at: created_at_u64,
        },
    })
}

fn required_str<'a>(v: &'a serde_json::Value, field: &str) -> Result<&'a str, SignerError> {
    v.get(field)
        .and_then(|x| x.as_str())
        .ok_or_else(|| SignerError::Backend(format!("missing {field}")))
}

fn parse_tag_rows(v: &serde_json::Value) -> Result<Vec<Vec<String>>, SignerError> {
    let arr = v
        .get("tags")
        .and_then(|x| x.as_array())
        .ok_or_else(|| SignerError::Backend("missing tags".to_string()))?;
    let mut out = Vec::with_capacity(arr.len());
    for (row_idx, row) in arr.iter().enumerate() {
        let row_arr = row.as_array().ok_or_else(|| {
            SignerError::Backend(format!("tag row {row_idx} is not an array"))
        })?;
        let mut row_out = Vec::with_capacity(row_arr.len());
        for (col_idx, cell) in row_arr.iter().enumerate() {
            let s = cell.as_str().ok_or_else(|| {
                SignerError::Backend(format!(
                    "tag row {row_idx} col {col_idx} is not a string"
                ))
            })?;
            row_out.push(s.to_string());
        }
        out.push(row_out);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn build_event_for_verify(
    id_hex: &str,
    pubkey: PublicKey,
    created_at: u64,
    kind: u64,
    tag_rows: &[Vec<String>],
    content: &str,
    sig_hex: &str,
) -> Result<Event, SignerError> {
    let id = EventId::from_hex(id_hex).map_err(|e| {
        SignerError::SignatureVerificationFailed(format!("invalid event id hex: {e}"))
    })?;
    let sig = Signature::from_str(sig_hex).map_err(|e| {
        SignerError::SignatureVerificationFailed(format!("invalid sig hex: {e}"))
    })?;
    let kind_u16 = u16::try_from(kind).map_err(|_| {
        SignerError::Backend(format!("kind {kind} does not fit in nostr u16 kind space"))
    })?;
    let kind = Kind::from_u16(kind_u16);
    let parsed_tags: Vec<Tag> = tag_rows
        .iter()
        .map(Tag::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            SignerError::SignatureVerificationFailed(format!("invalid tag in response: {e}"))
        })?;
    Ok(Event::new(
        id,
        pubkey,
        Timestamp::from(created_at),
        kind,
        parsed_tags,
        content.to_string(),
        sig,
    ))
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
