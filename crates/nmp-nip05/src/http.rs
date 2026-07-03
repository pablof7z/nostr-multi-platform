//! Blocking NIP-05 `.well-known/nostr.json` HTTP round-trip (native only).
//!
//! Runs on the spawned worker thread — blocking I/O is acceptable here
//! precisely because we are NOT on the actor thread (D8). Mirrors the
//! nmp-nip57 LNURL fetcher's `http_get_json` discipline: a bounded timeout
//! and a hard response-body cap so a hostile / runaway endpoint is a bounded
//! error, not an OOM event.

use nmp_wellknown_http::{assert_host_is_public, http_get_json};

/// Maximum response body the worker will accept. A NIP-05 `nostr.json`
/// document is a small JSON object (a `names` map, optionally a `relays`
/// map); 100 KiB is several orders of magnitude over any sane file. The cap
/// makes a hostile / runaway endpoint a bounded error rather than an OOM.
const NIP05_MAX_RESPONSE_BYTES: usize = 100 * 1024;

/// Fetch a domain's `https://<domain>/.well-known/nostr.json?name=<name>`
/// document and return the hex pubkey mapped to `name` in its `names` object.
///
/// `name` and `domain` are the already-shape-validated, lowercased parts from
/// [`crate::parse_nip05`]. The query is built here (not by the caller) so the
/// URL never carries an un-validated local-part.
///
/// Returns:
/// * `Ok(pubkey_hex)` — `names[name]` was present and is a 64-hex pubkey
///   (re-canonicalized through `nostr::PublicKey` so a mixed-case or
///   structurally-invalid value is rejected rather than forwarded).
/// * `Err(reason)` — the GET failed, the body was not JSON, `names` was
///   absent, `name` was not in `names`, or the mapped value was not a valid
///   pubkey. The reason is human-readable for the diagnostic toast; it never
///   echoes the response body verbatim.
pub fn resolve_nip05_pubkey_blocking(name: &str, domain: &str) -> Result<String, String> {
    // SSRF guard — reject IP-literal hosts and hosts that resolve to a
    // non-public address (loopback / private / link-local / unique-local /
    // CGNAT) BEFORE the fetch, so a NIP-05 identifier can't be used to probe
    // internal services. Runs here (on the blocking worker, not the actor
    // thread) because DNS resolution is itself blocking IO (D8).
    assert_host_is_public(domain)?;
    // Build the URL from the validated parts. `name` is restricted to the
    // NIP-05 local-part charset (`a-z0-9-_.`) by `parse_nip05`, so it needs no
    // percent-encoding; `domain` is a validated host. A literal `_` (the
    // NIP-05 root identifier) is queried verbatim per the spec.
    let url = format!("https://{domain}/.well-known/nostr.json?name={name}");
    let document = http_get_json(&url, NIP05_MAX_RESPONSE_BYTES)?;
    pubkey_from_names(&document, name)
}

/// Extract `names[name]` from a parsed `nostr.json` document and re-canonicalize
/// it through `nostr::PublicKey`. Split out from the HTTP fetch so it is unit
/// testable without a network.
pub(crate) fn pubkey_from_names(
    document: &serde_json::Value,
    name: &str,
) -> Result<String, String> {
    let names = document
        .get("names")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            "NIP-05 document is missing the `names` object (not a NIP-05 endpoint)".to_string()
        })?;
    let raw = names
        .get(name)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("NIP-05 document does not map `{name}` to a pubkey"))?
        .trim();
    nostr::PublicKey::from_hex(raw)
        .map(|pk| pk.to_hex())
        .map_err(|e| format!("NIP-05 document mapped `{name}` to an invalid pubkey: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_canonical_pubkey_for_name() {
        let pubkey = "b".repeat(64);
        let doc = serde_json::json!({ "names": { "alice": pubkey } });
        assert_eq!(pubkey_from_names(&doc, "alice").unwrap(), "b".repeat(64));
    }

    #[test]
    fn root_identifier_name_resolves() {
        let pubkey = "c".repeat(64);
        let doc = serde_json::json!({ "names": { "_": pubkey } });
        assert_eq!(pubkey_from_names(&doc, "_").unwrap(), "c".repeat(64));
    }

    #[test]
    fn missing_names_object_is_error() {
        let doc = serde_json::json!({ "relays": {} });
        assert!(pubkey_from_names(&doc, "alice").is_err());
    }

    #[test]
    fn name_absent_from_names_is_error() {
        let doc = serde_json::json!({ "names": { "bob": "d".repeat(64) } });
        assert!(pubkey_from_names(&doc, "alice").is_err());
    }

    #[test]
    fn invalid_pubkey_value_is_error() {
        let doc = serde_json::json!({ "names": { "alice": "not-a-pubkey" } });
        assert!(pubkey_from_names(&doc, "alice").is_err());
    }
}
