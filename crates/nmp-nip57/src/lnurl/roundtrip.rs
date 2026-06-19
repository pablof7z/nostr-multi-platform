//! Blocking LNURL-pay HTTP round-trip for the NIP-57 zap worker.

use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use nostr::Keys;

use super::{
    lnurl_to_well_known_url, looks_like_bolt11, metadata, pay, sign_zap_request, url_encode_query,
    validate_bolt11_amount, validate_description_hash, LnurlInvoice,
};

/// LNURL-pay total budget for the two-leg HTTP round-trip
/// (well-known fetch + callback fetch). Conservative — keeps a stuck
/// LN provider from accumulating worker threads even though each thread
/// is independent of the actor loop.
const LNURL_HTTP_TIMEOUT_SECS: u64 = 10;

/// Maximum response body the worker will accept from either LNURL hop.
/// LNURL-pay responses are tiny JSON objects (a few hundred bytes); 64 KiB
/// is several orders of magnitude over the spec. The cap exists to make a
/// hostile / runaway endpoint a bounded error, not an OOM event.
const LNURL_MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Two-leg LNURL-pay HTTP round-trip. Runs on the spawned worker thread —
/// blocking I/O is acceptable here precisely because we are NOT on the
/// actor thread. Also usable from standalone tools (see `fetch_bolt11_for_zap`).
pub(crate) fn fetch_lnurl_invoice_blocking(
    lnurl_or_address: &str,
    amount_msats: u64,
    signed_zap_request_json: &str,
) -> Result<LnurlInvoice, String> {
    let well_known_url = lnurl_to_well_known_url(lnurl_or_address)?;

    // Leg 1: well-known fetch. Pull the LNURL-pay metadata. We care about
    // `callback`, `minSendable`, `maxSendable`, and `allowsNostr` (must be
    // truthy for NIP-57).
    let well_known = http_get_json(&well_known_url)?;
    let callback = well_known
        .get("callback")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            "LNURL well-known response missing `callback` URL — receiver is not LNURL-pay enabled"
                .to_string()
        })?;
    if let Some(min) = well_known
        .get("minSendable")
        .and_then(serde_json::Value::as_u64)
    {
        if amount_msats < min {
            return Err(format!(
                "amount {amount_msats} msats below receiver minSendable {min} msats"
            ));
        }
    }
    if let Some(max) = well_known
        .get("maxSendable")
        .and_then(serde_json::Value::as_u64)
    {
        if amount_msats > max {
            return Err(format!(
                "amount {amount_msats} msats above receiver maxSendable {max} msats"
            ));
        }
    }
    let provider_pubkey = metadata::nostr_provider_pubkey(&well_known)?;

    // Leg 2: callback fetch. NIP-57 § "Appendix C" — append `amount` (msats)
    // and the URL-encoded signed kind:9734 as `nostr`. The response carries
    // the bolt11 in the `pr` field.
    //
    // NIP-57 Appendix B also specifies a `lnurl=<bech32>` query parameter
    // so the LN provider can associate the payment with the right Nostr
    // account and publish the kind:9735 receipt. Primal requires this.
    if !callback.starts_with("https://") {
        return Err(format!(
            "LNURL callback URL is not https:// (got: {callback})"
        ));
    }
    let separator = if callback.contains('?') { '&' } else { '?' };
    // Encode the well-known URL as bech32 for the `lnurl=` callback param.
    // If encoding fails we still attempt the request — some providers omit
    // the check, and failing silently here is preferable to aborting the zap.
    let lnurl_param = pay::url_to_bech32_lnurl(&well_known_url)
        .map(|b| format!("&lnurl={}", url_encode_query(&b)))
        .unwrap_or_default();
    let callback_url = format!(
        "{callback}{separator}amount={amount_msats}&nostr={}{lnurl_param}",
        url_encode_query(signed_zap_request_json),
    );
    let callback_response = http_get_json(&callback_url)?;

    // LUD-06 says a successful response is `{ "pr": "lnbc…" }`; an error
    // shape is `{ "status": "ERROR", "reason": "…" }`. Handle the error
    // shape so the user sees the provider's reason rather than a generic
    // "missing pr field".
    if let Some(status) = callback_response
        .get("status")
        .and_then(serde_json::Value::as_str)
    {
        if status.eq_ignore_ascii_case("ERROR") {
            let reason = callback_response
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("LNURL provider returned ERROR without a reason");
            return Err(format!("LNURL provider error: {reason}"));
        }
    }
    let bolt11 = callback_response
        .get("pr")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "LNURL callback response missing `pr` (bolt11 invoice) field".to_string())?;
    if !looks_like_bolt11(bolt11) {
        return Err(format!(
            "LNURL callback returned a `pr` value that does not look like a bolt11 invoice: {bolt11}"
        ));
    }
    // NIP-57 recommendation — verify the bolt11 encodes exactly the amount the
    // user requested before handing it to the wallet for automatic payment.
    // Fail closed: an amountless invoice or one with a different amount is
    // never forwarded to the wallet (D6 — errors as state, not panic).
    validate_bolt11_amount(bolt11, amount_msats)?;
    // NIP-57 commitment check (zap mode only) — binds the invoice to *our*
    // signed request. Runs before return, hence before any pay dispatch.
    validate_description_hash(bolt11, signed_zap_request_json, true)?;
    Ok(LnurlInvoice {
        bolt11: bolt11.to_string(),
        provider_pubkey,
    })
}

/// One-shot HTTP GET → JSON. Bounded by `LNURL_HTTP_TIMEOUT_SECS` and
/// `LNURL_MAX_RESPONSE_BYTES`. The result is a `serde_json::Value` rather
/// than a typed shape because LNURL-pay returns a slightly different schema
/// per leg (well-known has `callback`/`minSendable`/…; callback has
/// `pr`/`status`/…), and the typed-shape boilerplate adds no safety here.
fn http_get_json(url: &str) -> Result<serde_json::Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(LNURL_HTTP_TIMEOUT_SECS))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("HTTP GET {url} failed: {e}"))?;
    if response.status() != 200 {
        return Err(format!(
            "HTTP GET {url} returned status {} {}",
            response.status(),
            response.status_text()
        ));
    }
    // Bound the response so a runaway/hostile endpoint can't OOM us.
    let mut body = Vec::with_capacity(1024);
    response
        .into_reader()
        .take(LNURL_MAX_RESPONSE_BYTES as u64)
        .read_to_end(&mut body)
        .map_err(|e| format!("read response body from {url}: {e}"))?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| format!("parse JSON from {url}: {e}"))
}

/// Standalone blocking entry point for one-shot tools and integration tests.
///
/// Unlike [`super::FetchLnurlInvoiceCommand`] (which runs inside the NMP actor
/// pipeline), this function blocks the calling thread directly. Use it from
/// CLI binaries and integration tests where the actor stack is not available.
///
/// Signs the kind:9734 zap request with `keys`, does the two-leg LNURL-pay
/// round-trip, and returns the bolt11 invoice string on success.
#[allow(dead_code)] // Reference impl retained for the zap-smoke tool's docs.
pub(crate) fn fetch_bolt11_for_zap(
    keys: &Keys,
    lnurl_or_address: &str,
    amount_msats: u64,
    recipient_pubkey: &str,
    relays: &[String],
    comment: Option<&str>,
) -> Result<String, String> {
    // NIP-57: the `lnurl` tag must be bech32-encoded (LUD-01), NOT the raw
    // lightning address. Resolve → well-known URL → bech32 before building.
    let well_known_url = lnurl_to_well_known_url(lnurl_or_address)?;
    let bech32_lnurl = pay::url_to_bech32_lnurl(&well_known_url)?;
    let mut builder = crate::build::ZapRequest::to_pubkey(recipient_pubkey)
        .amount_msats(amount_msats)
        .relays(relays.to_vec())
        .lnurl(&bech32_lnurl);
    if let Some(c) = comment {
        builder = builder.comment(c);
    }
    let mut unsigned = builder
        .build()
        .map_err(|e| format!("build kind:9734: {e}"))?;
    // D7 — this standalone path owns the wall clock directly (no actor context).
    // Re-stamp `created_at` the same way `FetchLnurlInvoiceCommand::run` does
    // for the actor path. `pubkey` is re-derived from `keys` inside
    // `sign_zap_request` via `EventBuilder::sign_with_keys`.
    unsigned.created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let signed_json = sign_zap_request(keys, &unsigned)?;
    fetch_lnurl_invoice_blocking(lnurl_or_address, amount_msats, &signed_json)
        .map(|invoice| invoice.bolt11)
}
