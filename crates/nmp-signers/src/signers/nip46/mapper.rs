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
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| SignerError::Backend("missing kind".to_string()))?;
    let created_at_u64 = v
        .get("created_at")
        .and_then(serde_json::Value::as_u64)
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

#[cfg(test)]
mod tests {
    //! Coverage for the NIP-46 response mapper.
    //!
    //! `parse_signed_event_response` is the inbound trust boundary: every byte
    //! comes from a remote bunker that may be buggy or malicious.  Per **D6**
    //! every malformed shape must return `Err`, never panic.  The happy path
    //! and the three forgery cases (tampered sig / swapped content / wrong
    //! pubkey) are covered by `tests/codex_9944bed_followup.rs`; this module
    //! pins the *structural* failure matrix (missing / wrong-typed fields) that
    //! the verify()-based tests never exercise because they never reach
    //! `verify()`.

    use super::*;
    use crate::signers::traits::Signer;
    use crate::LocalKeySigner;

    /// A real keypair + a self-consistent JSON response body for it.  Reused so
    /// each failure test starts from a *valid* body and mutates exactly one
    /// field — proving the rejection is caused by that field alone.
    fn valid_response() -> (PublicKey, String) {
        let signer = LocalKeySigner::generate();
        let pubkey = signer.pubkey();
        let unsigned = UnsignedEvent {
            pubkey: pubkey.to_hex(),
            kind: 1,
            tags: vec![vec!["t".to_string(), "x".to_string()]],
            content: "mapper test".to_string(),
            created_at: 1_700_000_000,
        };
        let signed = <LocalKeySigner as Signer>::sign(&signer, unsigned)
            .wait(std::time::Duration::from_secs(1))
            .expect("real sign");
        let body = format!(
            r#"{{"id":"{}","pubkey":"{}","sig":"{}","kind":{},"created_at":{},"tags":[["t","x"]],"content":"{}"}}"#,
            signed.id,
            signed.unsigned.pubkey,
            signed.sig,
            signed.unsigned.kind,
            signed.unsigned.created_at,
            signed.unsigned.content,
        );
        (pubkey, body)
    }

    #[test]
    fn valid_response_round_trips() {
        // Baseline: the helper actually produces an acceptable body.  Without
        // this, a green "rejects X" suite could be passing because *every*
        // input fails for an unrelated reason.
        let (pk, body) = valid_response();
        let signed = parse_signed_event_response(&body, pk).expect("valid body must parse");
        assert_eq!(signed.unsigned.pubkey, pk.to_hex());
        assert_eq!(signed.unsigned.content, "mapper test");
        assert_eq!(signed.unsigned.tags, vec![vec!["t".to_string(), "x".to_string()]]);
    }

    #[test]
    fn non_json_body_returns_backend_err() {
        let (pk, _) = valid_response();
        let err = parse_signed_event_response("not json {{", pk).expect_err("must reject");
        assert!(matches!(err, SignerError::Backend(m) if m.contains("not JSON")));
    }

    #[test]
    fn missing_required_string_fields_return_backend_err() {
        // Drop each required string field in turn — each must surface as a
        // Backend error naming the field, never a panic.
        let (pk, body) = valid_response();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        for field in ["pubkey", "content", "id", "sig"] {
            let mut obj = v.as_object().unwrap().clone();
            obj.remove(field);
            let mutated = serde_json::Value::Object(obj).to_string();
            match parse_signed_event_response(&mutated, pk) {
                Err(SignerError::Backend(m)) => {
                    assert!(m.contains(field), "error for missing {field} should name it: {m}");
                }
                Err(SignerError::Mismatch(_)) if field == "pubkey" => {
                    // `pubkey` absent is also acceptably surfaced as the
                    // identity check failing — still an Err, still no panic.
                }
                other => panic!("missing {field}: expected Backend Err, got {other:?}"),
            }
        }
    }

    #[test]
    fn missing_numeric_and_tag_fields_return_backend_err() {
        let (pk, body) = valid_response();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        for field in ["kind", "created_at", "tags"] {
            let mut obj = v.as_object().unwrap().clone();
            obj.remove(field);
            let mutated = serde_json::Value::Object(obj).to_string();
            match parse_signed_event_response(&mutated, pk) {
                Err(SignerError::Backend(m)) => {
                    assert!(m.contains(field), "error for missing {field} should name it: {m}");
                }
                other => panic!("missing {field}: expected Backend Err, got {other:?}"),
            }
        }
    }

    #[test]
    fn tags_not_an_array_returns_backend_err() {
        let (pk, body) = valid_response();
        let mutated = body.replace(r#""tags":[["t","x"]]"#, r#""tags":"oops""#);
        assert!(
            matches!(parse_signed_event_response(&mutated, pk), Err(SignerError::Backend(_))),
            "non-array tags must be Backend Err"
        );
    }

    #[test]
    fn tag_row_not_an_array_returns_backend_err() {
        let (pk, body) = valid_response();
        let mutated = body.replace(r#""tags":[["t","x"]]"#, r#""tags":["scalar"]"#);
        match parse_signed_event_response(&mutated, pk) {
            Err(SignerError::Backend(m)) => assert!(m.contains("row")),
            other => panic!("expected Backend(row) Err, got {other:?}"),
        }
    }

    #[test]
    fn tag_cell_not_a_string_returns_backend_err() {
        let (pk, body) = valid_response();
        let mutated = body.replace(r#""tags":[["t","x"]]"#, r#""tags":[["t",42]]"#);
        match parse_signed_event_response(&mutated, pk) {
            Err(SignerError::Backend(m)) => assert!(m.contains("col")),
            other => panic!("expected Backend(col) Err, got {other:?}"),
        }
    }

    #[test]
    fn invalid_id_hex_returns_verification_failed() {
        let (pk, body) = valid_response();
        // Replace the id with a syntactically-wrong-length hex string.
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let mut obj = v.as_object().unwrap().clone();
        obj.insert("id".to_string(), serde_json::json!("zzz"));
        let mutated = serde_json::Value::Object(obj).to_string();
        match parse_signed_event_response(&mutated, pk) {
            Err(SignerError::SignatureVerificationFailed(m)) => {
                assert!(m.contains("event id"));
            }
            other => panic!("expected SignatureVerificationFailed, got {other:?}"),
        }
    }

    #[test]
    fn invalid_sig_hex_returns_verification_failed() {
        let (pk, body) = valid_response();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let mut obj = v.as_object().unwrap().clone();
        obj.insert("sig".to_string(), serde_json::json!("nothex"));
        let mutated = serde_json::Value::Object(obj).to_string();
        match parse_signed_event_response(&mutated, pk) {
            Err(SignerError::SignatureVerificationFailed(m)) => {
                assert!(m.contains("sig"));
            }
            other => panic!("expected SignatureVerificationFailed, got {other:?}"),
        }
    }

    #[test]
    fn kind_outside_u16_space_returns_backend_err() {
        // nostr's `Kind` is a u16; a response claiming kind 70000 must be
        // refused with a structured Backend error, not an arithmetic panic.
        let (pk, body) = valid_response();
        let mutated = body.replace(r#""kind":1"#, r#""kind":70000"#);
        match parse_signed_event_response(&mutated, pk) {
            Err(SignerError::Backend(m)) => assert!(m.contains("kind")),
            other => panic!("expected Backend(kind) Err, got {other:?}"),
        }
    }

    #[test]
    fn negative_kind_is_not_a_u64_and_returns_backend_err() {
        // A negative kind is not representable as u64 — `as_u64()` yields None,
        // so it surfaces as the "missing kind" path.  Still an Err, no panic.
        let (pk, body) = valid_response();
        let mutated = body.replace(r#""kind":1"#, r#""kind":-1"#);
        assert!(matches!(
            parse_signed_event_response(&mutated, pk),
            Err(SignerError::Backend(_))
        ));
    }

    #[test]
    fn map_response_to_event_passes_through_inner_error() {
        // A `Ready(Err(..))` raw op must surface unchanged — the mapper does
        // not wrap or swallow an upstream transport error.
        let pk = LocalKeySigner::generate().pubkey();
        let unsigned = UnsignedEvent {
            pubkey: pk.to_hex(),
            kind: 1,
            tags: vec![],
            content: "x".to_string(),
            created_at: 1,
        };
        let raw = SignerOp::err(SignerError::Rejected("upstream said no".to_string()));
        let mapped = map_response_to_event(raw, unsigned, pk);
        match mapped.wait(std::time::Duration::from_millis(50)) {
            Err(SignerError::Rejected(m)) => assert_eq!(m, "upstream said no"),
            other => panic!("expected Rejected passthrough, got {other:?}"),
        }
    }

    #[test]
    fn map_response_to_event_pending_channel_disconnect_is_backend_err() {
        // If the pending channel's sender is dropped without a value, the
        // mapper's worker thread must convert the disconnect into a Backend
        // error rather than hang or panic.
        let pk = LocalKeySigner::generate().pubkey();
        let unsigned = UnsignedEvent {
            pubkey: pk.to_hex(),
            kind: 1,
            tags: vec![],
            content: "x".to_string(),
            created_at: 1,
        };
        let (tx, rx) = std::sync::mpsc::channel::<Result<String, SignerError>>();
        let mapped = map_response_to_event(SignerOp::Pending(rx), unsigned, pk);
        drop(tx); // sender gone, no value ever sent
        match mapped.wait(std::time::Duration::from_secs(1)) {
            Err(SignerError::Backend(m)) => assert!(m.contains("disconnected")),
            other => panic!("expected Backend(disconnected), got {other:?}"),
        }
    }

    #[test]
    fn escape_json_escapes_control_and_quote_chars() {
        assert_eq!(escape_json(r#"a"b"#), r#"a\"b"#);
        assert_eq!(escape_json(r"a\b"), r"a\\b");
        assert_eq!(escape_json("a\nb"), r"a\nb");
        assert_eq!(escape_json("a\rb"), r"a\rb");
        assert_eq!(escape_json("a\tb"), r"a\tb");
        // A sub-0x20 control char → \uXXXX form.
        assert_eq!(escape_json("\u{0001}"), "\\u0001");
    }

    #[test]
    fn escape_json_passes_through_safe_and_unicode_chars() {
        assert_eq!(escape_json("plain text 123"), "plain text 123");
        // Multi-byte unicode is passed through verbatim — NIP-46 params are
        // UTF-8 JSON strings; only structural chars need escaping.
        assert_eq!(escape_json("café 🚀"), "café 🚀");
        assert_eq!(escape_json(""), "");
    }

    #[test]
    fn escape_json_output_is_valid_json_string_content() {
        // Round-trip: wrapping the escaped output in quotes must parse back to
        // the original — the whole point of escaping.
        for input in ["", "simple", "with \"quotes\"", "tab\there", "new\nline", "\u{0007}bell"] {
            let wrapped = format!("\"{}\"", escape_json(input));
            let decoded: String =
                serde_json::from_str(&wrapped).expect("escaped output must be valid JSON");
            assert_eq!(decoded, input, "round-trip mismatch for {input:?}");
        }
    }
}
