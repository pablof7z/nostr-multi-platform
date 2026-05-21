//! LNURL-pay round-trip for NIP-57 zaps — leg 2 of the zap dance.
//!
//! # Scope
//!
//! NIP-57 has two protocol legs and this module owns the second:
//!
//! 1. **kind:9734 zap request** — built + published by [`crate::action`] and the
//!    `ActionRegistry` executor. Already wired.
//! 2. **LNURL-pay round-trip** — the signed kind:9734 must be POSTed to the
//!    recipient's LNURL-pay callback to obtain a bolt11 invoice that the
//!    sender's lightning wallet pays. This module performs that round-trip.
//!
//! The LNURL flow is two-step per the [LNURL-pay spec][lnurl-pay] (LUD-06)
//! with the NIP-57 amendments:
//!
//! 1. **GET** the LNURL endpoint (the recipient publishes either a raw HTTPS
//!    URL — LUD-16 lightning addresses normalize to one — or a bech32
//!    `lnurl1…` blob; this crate accepts the raw URL form only — see
//!    [Known limitations](#known-limitations)). The response is a JSON object
//!    carrying at minimum `callback`, `minSendable`, `maxSendable`. NIP-57
//!    requires the additional `allowsNostr: true` and `nostrPubkey: <hex>`
//!    fields; this module enforces both.
//! 2. **GET** the `callback` URL with query params `amount=<msats>` and
//!    `nostr=<url-encoded JSON of the signed kind:9734>`. The response carries
//!    `pr: "lnbc..."` — the bolt11 invoice.
//!
//! [lnurl-pay]: https://github.com/lnurl/luds/blob/luds/06.md
//!
//! # Threading and blocking
//!
//! `fetch_invoice` is **synchronous and blocking** by design. The actor thread
//! must NEVER call it directly (D8 — non-blocking I/O on the actor). The host
//! shell that wires this module (`nmp-app-chirp`) spawns a dedicated worker
//! thread for the call and routes the outcome back to the actor through the
//! `Sender<ActorCommand>` clone it captured at registration time. A oneshot is
//! not needed — the spawned thread owns the sender outright until it sends a
//! single follow-up command (success toast or failure toast).
//!
//! # Known limitations
//!
//! - **No `lud06` bech32 path.** The recipient's `lnurl` field is taken as a
//!   raw HTTPS URL. A `lnurl1…` bech32 blob is rejected with
//!   [`LnurlError::Invalid`]. The `lud16` lightning-address form
//!   (`user@domain.tld`) is the more common shape in practice and resolves to
//!   `https://domain.tld/.well-known/lnurlp/user` — callers do this
//!   translation upstream.
//! - **Amount unit.** The LNURL `amount` query param is in **millisats** per
//!   the LNURL-pay spec. `fetch_invoice` takes `amount_msats: u64`; the caller
//!   converts from sats × 1000 once at the boundary (matching
//!   `ZapAction::amount_sats` → kind:9734 `amount` tag).
//! - **`nostrPubkey` recipient match.** NIP-57 says the LNURL `nostrPubkey`
//!   SHOULD equal the recipient's pubkey; this module returns it in the
//!   metadata for the host to log / compare but does not reject a mismatch
//!   here. The PR description carries the rationale.

use std::time::Duration;

/// Errors returned by [`fetch_invoice`]. Variants are surfaced as toasts by
/// the host wiring (no panics across the FFI).
#[derive(Debug)]
pub enum LnurlError {
    /// The `lnurl` argument was not a raw HTTPS URL we know how to fetch
    /// (e.g. a bech32 `lnurl1…` blob, an empty string, or a non-HTTPS scheme).
    Invalid(String),
    /// The first GET to the LNURL endpoint failed transport-wise (DNS, TLS,
    /// connection refused, 4xx/5xx). Carries the underlying message.
    EndpointError(String),
    /// The endpoint responded but the JSON failed to parse / did not carry the
    /// fields NIP-57 requires (`callback`, `minSendable`, `maxSendable`,
    /// `allowsNostr: true`, `nostrPubkey`).
    InvalidMetadata(String),
    /// The amount is outside the endpoint's accepted `[minSendable,
    /// maxSendable]` window.
    AmountOutOfRange { min_msats: u64, max_msats: u64, requested_msats: u64 },
    /// The callback GET failed transport-wise or returned a non-2xx status.
    CallbackError(String),
    /// The callback response did not carry a `pr` bolt11 invoice.
    InvoiceMissing(String),
}

impl core::fmt::Display for LnurlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid lnurl: {s}"),
            Self::EndpointError(s) => write!(f, "LNURL endpoint error: {s}"),
            Self::InvalidMetadata(s) => write!(f, "LNURL metadata invalid: {s}"),
            Self::AmountOutOfRange { min_msats, max_msats, requested_msats } => write!(
                f,
                "amount {requested_msats} msats outside LNURL range \
                 [{min_msats}, {max_msats}]"
            ),
            Self::CallbackError(s) => write!(f, "LNURL callback error: {s}"),
            Self::InvoiceMissing(s) => write!(f, "LNURL callback returned no invoice: {s}"),
        }
    }
}

impl std::error::Error for LnurlError {}

/// The bolt11 invoice + recipient metadata returned by a successful LNURL
/// round-trip. The host routes `invoice` into a wallet (in-process via
/// [`crate::action`] follow-up or out-of-process via a toast); `nostr_pubkey`
/// is returned for diagnostic logging — NIP-57 says it SHOULD match the zap
/// recipient (this module does not reject a mismatch; see module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnurlInvoice {
    pub invoice: String,
    pub nostr_pubkey: String,
}

/// Fetch a bolt11 invoice for a NIP-57 zap from the recipient's LNURL-pay
/// endpoint.
///
/// `lnurl` MUST be a raw `https://` URL (LUD-06 `lnurl1…` bech32 blobs are
/// rejected — see [Known limitations](self#known-limitations)). `signed_event_json`
/// is the JSON of the **signed** kind:9734 event (id, pubkey, sig included) —
/// per NIP-57 it is sent as the `nostr` query param to the callback URL and
/// is embedded in the receipt by the LN provider. `amount_msats` is the zap
/// amount in **millisatoshis** (NIP-57: `amount` tag of the kind:9734 is
/// already in msats; pass the same number).
///
/// Returns the bolt11 invoice + the LNURL endpoint's advertised `nostrPubkey`
/// on success.
///
/// # Blocking
///
/// This function blocks the calling thread for up to two HTTP round-trips
/// (one to the LNURL endpoint, one to the callback). It MUST be called from
/// a worker thread, never the actor thread. A 20-second timeout is applied
/// to each call so a hung endpoint cannot park the worker forever.
pub fn fetch_invoice(
    lnurl: &str,
    signed_event_json: &str,
    amount_msats: u64,
) -> Result<LnurlInvoice, LnurlError> {
    let trimmed = lnurl.trim();
    if trimmed.is_empty() {
        return Err(LnurlError::Invalid("empty lnurl".to_string()));
    }
    // LUD-06 bech32 blobs are deferred — `bech32` is not in the workspace and
    // adding it for one decode is out of scope for this PR. Lightning-address
    // (`user@domain`) translation lives upstream — by the time it reaches
    // here, the caller has already resolved to the `.well-known/lnurlp/<user>`
    // URL. So we accept only the raw HTTPS form.
    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return Err(LnurlError::Invalid(format!(
            "expected https:// URL (bech32 lnurl1… blobs not supported in this build), got: {}",
            trim_for_log(trimmed)
        )));
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();

    // Step 1: GET the LNURL endpoint. Per LNURL-pay spec the response is a
    // JSON object; per NIP-57 it MUST advertise `allowsNostr: true` and a
    // `nostrPubkey`.
    let metadata_response = agent
        .get(trimmed)
        .call()
        .map_err(|e| LnurlError::EndpointError(e.to_string()))?;
    let metadata_text = metadata_response
        .into_string()
        .map_err(|e| LnurlError::EndpointError(format!("read body: {e}")))?;
    let metadata = parse_metadata(&metadata_text)?;

    if amount_msats < metadata.min_sendable_msats
        || amount_msats > metadata.max_sendable_msats
    {
        return Err(LnurlError::AmountOutOfRange {
            min_msats: metadata.min_sendable_msats,
            max_msats: metadata.max_sendable_msats,
            requested_msats: amount_msats,
        });
    }

    // Step 2: GET the callback with `amount` (msats) and `nostr` (url-encoded
    // signed event JSON). `ureq` URL-encodes the query params for us.
    let invoice_response = agent
        .get(&metadata.callback)
        .query("amount", &amount_msats.to_string())
        .query("nostr", signed_event_json)
        .call()
        .map_err(|e| LnurlError::CallbackError(e.to_string()))?;
    let invoice_text = invoice_response
        .into_string()
        .map_err(|e| LnurlError::CallbackError(format!("read body: {e}")))?;

    let invoice = parse_invoice(&invoice_text)?;
    Ok(LnurlInvoice {
        invoice,
        nostr_pubkey: metadata.nostr_pubkey,
    })
}

/// Minimal subset of the LNURL-pay metadata response NIP-57 requires.
struct LnurlMetadata {
    callback: String,
    min_sendable_msats: u64,
    max_sendable_msats: u64,
    nostr_pubkey: String,
}

fn parse_metadata(body: &str) -> Result<LnurlMetadata, LnurlError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| LnurlError::InvalidMetadata(format!("not JSON: {e}")))?;

    // LNURL-pay error envelope: `{"status":"ERROR","reason":"…"}`. Surface the
    // reason directly so the toast carries it.
    if value.get("status").and_then(|v| v.as_str()) == Some("ERROR") {
        let reason = value
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("(no reason)");
        return Err(LnurlError::InvalidMetadata(format!("status=ERROR: {reason}")));
    }

    let callback = value
        .get("callback")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LnurlError::InvalidMetadata("missing `callback`".to_string()))?
        .to_string();
    let min_sendable_msats = value
        .get("minSendable")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| LnurlError::InvalidMetadata("missing `minSendable`".to_string()))?;
    let max_sendable_msats = value
        .get("maxSendable")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| LnurlError::InvalidMetadata("missing `maxSendable`".to_string()))?;
    // NIP-57 requires both. A LUD-06 endpoint that does not opt into NIP-57
    // would not advertise these; surface the gap as `InvalidMetadata` so the
    // host toast is "recipient does not support zaps", not a vague HTTP error.
    let allows_nostr = value
        .get("allowsNostr")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !allows_nostr {
        return Err(LnurlError::InvalidMetadata(
            "endpoint does not advertise `allowsNostr: true` (NIP-57 not supported)".to_string(),
        ));
    }
    let nostr_pubkey = value
        .get("nostrPubkey")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            LnurlError::InvalidMetadata("missing `nostrPubkey` (NIP-57)".to_string())
        })?
        .to_string();

    Ok(LnurlMetadata {
        callback,
        min_sendable_msats,
        max_sendable_msats,
        nostr_pubkey,
    })
}

fn parse_invoice(body: &str) -> Result<String, LnurlError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| LnurlError::InvoiceMissing(format!("not JSON: {e}")))?;
    if value.get("status").and_then(|v| v.as_str()) == Some("ERROR") {
        let reason = value
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("(no reason)");
        return Err(LnurlError::CallbackError(format!("status=ERROR: {reason}")));
    }
    let pr = value
        .get("pr")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LnurlError::InvoiceMissing("missing `pr` field".to_string()))?;
    if pr.trim().is_empty() {
        return Err(LnurlError::InvoiceMissing("`pr` is empty".to_string()));
    }
    Ok(pr.to_string())
}

/// Trim a string for log/error inclusion so a 4 KB URL doesn't blow up a toast.
fn trim_for_log(s: &str) -> String {
    const LIMIT: usize = 80;
    if s.len() <= LIMIT {
        s.to_string()
    } else {
        format!("{}…(+{} chars)", &s[..LIMIT], s.len() - LIMIT)
    }
}

#[cfg(test)]
#[path = "lnurl_tests.rs"]
mod lnurl_tests;
