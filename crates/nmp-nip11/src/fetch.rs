//! Blocking NIP-11 HTTP fetch. Runs ONLY on a spawned worker thread (never the
//! actor thread, D8) — mirrors `nmp-nip57`'s LNURL fetcher and `nmp-blossom`'s
//! upload transport: a `ureq` agent with a per-request timeout and a
//! max-response-bytes cap so a hostile / runaway relay is a bounded error, not
//! an OOM or a wedged worker.

use std::io::Read;
use std::time::Duration;

use nmp_core::substrate::RelayInfoDoc;

use crate::parse::parse_relay_info;
use crate::url::http_url_for_relay;

/// Per-fetch HTTP budget. NIP-11 documents are tiny; a relay that does not
/// answer within this window simply has no document this cycle.
const FETCH_TIMEOUT_SECS: u64 = 10;

/// Maximum NIP-11 body the worker accepts. Documents are a few hundred bytes to
/// a few KiB; 64 KiB is orders of magnitude over the spec, matching the LNURL /
/// Blossom caps. The cap makes a hostile / runaway response a bounded error.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// The `Accept` header NIP-11 mandates so the relay returns its information
/// document instead of attempting a WebSocket upgrade.
const NOSTR_JSON_ACCEPT: &str = "application/nostr+json";

/// Fetch and parse a relay's NIP-11 information document. BLOCKING — call only
/// from a spawned worker thread.
///
/// Maps the `wss://`/`ws://` URL to its HTTP form, issues a `GET` with
/// `Accept: application/nostr+json`, caps the body, and parses it into a
/// [`RelayInfoDoc`] tagged with the original `relay_url`.
///
/// Returns an error string on any failure (unmappable URL, transport error,
/// non-2xx status, oversized/garbage body). Callers treat an error as "this
/// relay has no document" — it is always non-fatal.
pub fn fetch_relay_info_blocking(relay_url: &str) -> Result<RelayInfoDoc, String> {
    let http_url = http_url_for_relay(relay_url)
        .ok_or_else(|| format!("cannot map relay URL to an HTTP URL: {relay_url}"))?;

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build();
    let response = agent
        .get(&http_url)
        .set("Accept", NOSTR_JSON_ACCEPT)
        .call()
        .map_err(|e| format!("NIP-11 GET {http_url} failed: {e}"))?;
    if response.status() != 200 {
        return Err(format!(
            "NIP-11 GET {http_url} returned status {} {}",
            response.status(),
            response.status_text()
        ));
    }
    let mut body = Vec::with_capacity(1024);
    response
        .into_reader()
        .take(MAX_RESPONSE_BYTES as u64)
        .read_to_end(&mut body)
        .map_err(|e| format!("read NIP-11 body from {http_url}: {e}"))?;

    parse_relay_info(relay_url, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmappable_url_is_an_error_without_network() {
        // A bare host has no scheme to map; the fetch errors before any socket.
        assert!(fetch_relay_info_blocking("relay.example").is_err());
    }
}
