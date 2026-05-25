//! LNURL-pay decode + query-encode helpers used by
//! [`super::FetchLnurlInvoiceCommand`].
//!
//! Split out of `lnurl::mod` so the orchestrator file stays under the
//! 500-LOC hard cap (file-size gate). All functions here are pure (no I/O,
//! no mutable state) and unit-tested in isolation.

/// Convert any of the three LNURL-pay input shapes into the well-known URL
/// to GET.
///
/// 1. **Lightning address** (`user@domain`) — convert per LUD-16 to
///    `https://<domain>/.well-known/lnurlp/<user>`.
/// 2. **Bech32 LNURL** (`lnurl1…`) — decode per LUD-01 to the embedded URL
///    bytes and use directly.
/// 3. **Bare URL** (`https://…`) — pass through.
///
/// Any other shape is an `Err`.
pub fn lnurl_to_well_known_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("LNURL input is empty".to_string());
    }
    // Case 3 — bare URL. Accept https only; LN providers MUST use TLS.
    // (LUD-01 §1 references TLS; a plaintext fallback would leak the zap
    // amount + signed event content.)
    if let Some(rest) = trimmed.strip_prefix("https://") {
        if rest.is_empty() {
            return Err("LNURL URL has empty authority after scheme".to_string());
        }
        return Ok(trimmed.to_string());
    }
    if trimmed.starts_with("http://") {
        return Err(
            "LNURL input uses http:// — refusing for privacy reasons (TLS is mandatory per LUD-01)"
                .to_string(),
        );
    }
    // Case 2 — bech32 LNURL. Accept both upper- and lower-case (bech32 is
    // case-insensitive but HRP must agree).
    if trimmed.to_lowercase().starts_with("lnurl1") {
        return decode_bech32_lnurl(trimmed);
    }
    // Case 1 — lightning address. Per LUD-16 the local part is restricted
    // to a/A–z/Z, 0–9, -, _, ., +; bias toward the spec rather than allow
    // arbitrary characters that could inject path segments.
    if let Some((local, domain)) = trimmed.split_once('@') {
        if local.is_empty() || domain.is_empty() {
            return Err(format!("malformed lightning address: {trimmed}"));
        }
        if !local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        {
            return Err(format!(
                "lightning address local part contains characters not permitted by LUD-16: {local}"
            ));
        }
        if !domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':'))
        {
            return Err(format!(
                "lightning address domain contains characters not permitted in a hostname: {domain}"
            ));
        }
        return Ok(format!("https://{domain}/.well-known/lnurlp/{local}"));
    }
    Err(format!(
        "LNURL input is not a lightning address (user@domain), a bech32 lnurl1…, or a https URL: {trimmed}"
    ))
}

/// LUD-01 bech32 LNURL decode — strip the HRP, ungroup the 5-bit data, and
/// validate the result is a UTF-8 https URL.
fn decode_bech32_lnurl(input: &str) -> Result<String, String> {
    let (hrp, data) =
        bech32::decode(input).map_err(|e| format!("invalid bech32 LNURL: {e}"))?;
    if hrp.as_str().to_lowercase() != "lnurl" {
        return Err(format!(
            "bech32 HRP is {} (expected `lnurl`)",
            hrp.as_str()
        ));
    }
    let url = String::from_utf8(data)
        .map_err(|e| format!("decoded LNURL bytes are not valid UTF-8: {e}"))?;
    if !url.starts_with("https://") {
        return Err(format!(
            "decoded LNURL does not start with https:// (got: {url})"
        ));
    }
    Ok(url)
}

/// Percent-encode a query-string value per RFC 3986 §2.3 — unreserved
/// characters pass through verbatim, every other byte (including non-ASCII
/// UTF-8 continuation bytes) becomes `%XX`. Sufficient for the LUD-06
/// `nostr=<urlencoded-event>` parameter and the `amount=<msats>` parameter
/// (which is decimal-digits-only so it round-trips unchanged).
pub fn url_encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        // RFC 3986 unreserved set: ALPHA / DIGIT / "-" / "." / "_" / "~".
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// Cheap, permissive bolt11 sanity check — accepts mainnet (`lnbc`),
/// testnet (`lntb`), regtest (`lnbcrt`), and signet (`lntbs`) HRPs. A full
/// bolt11 parser lives in the `lightning-invoice` crate; for our purposes
/// the prefix check is enough to detect a clearly-wrong response (e.g. an
/// HTML error page leaking through a misconfigured status code).
pub fn looks_like_bolt11(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    lower.starts_with("lnbc")
        || lower.starts_with("lntb")
        || lower.starts_with("lnbcrt")
        || lower.starts_with("lntbs")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Lightning address → well-known URL (LUD-16).
    #[test]
    fn lightning_address_resolves_to_well_known_url() {
        assert_eq!(
            lnurl_to_well_known_url("alice@example.com").unwrap(),
            "https://example.com/.well-known/lnurlp/alice"
        );
    }

    #[test]
    fn lightning_address_with_plus_and_subdomain() {
        assert_eq!(
            lnurl_to_well_known_url("a.bob+test@ln.example.co").unwrap(),
            "https://ln.example.co/.well-known/lnurlp/a.bob+test"
        );
    }

    #[test]
    fn lightning_address_rejects_empty_local() {
        assert!(lnurl_to_well_known_url("@example.com").is_err());
    }

    #[test]
    fn lightning_address_rejects_empty_domain() {
        assert!(lnurl_to_well_known_url("alice@").is_err());
    }

    #[test]
    fn lightning_address_rejects_forbidden_chars() {
        // `/` would let a hostile input inject a path segment.
        assert!(lnurl_to_well_known_url("alice/etc@example.com").is_err());
    }

    // Bare URL pass-through.
    #[test]
    fn https_url_passes_through_unchanged() {
        let url = "https://example.com/.well-known/lnurlp/bob";
        assert_eq!(lnurl_to_well_known_url(url).unwrap(), url);
    }

    #[test]
    fn http_url_is_rejected() {
        assert!(lnurl_to_well_known_url("http://example.com/lnurlp").is_err());
    }

    // Bech32 LNURL decode (LUD-01). Round-trip-encode a known URL via
    // `bech32` to avoid pasting magic constants — the test then verifies
    // our decoder recovers it.
    #[test]
    fn bech32_lnurl_round_trips() {
        use bech32::{Bech32, Hrp};
        let original = "https://example.com/.well-known/lnurlp/charlie";
        let hrp = Hrp::parse("lnurl").unwrap();
        let encoded = bech32::encode::<Bech32>(hrp, original.as_bytes()).unwrap();
        assert!(encoded.to_lowercase().starts_with("lnurl1"));
        let decoded = lnurl_to_well_known_url(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn bech32_lnurl_rejects_wrong_hrp() {
        use bech32::{Bech32, Hrp};
        let hrp = Hrp::parse("npub").unwrap();
        let encoded = bech32::encode::<Bech32>(hrp, b"https://example.com").unwrap();
        // Doesn't start with `lnurl1`, so it falls through to the
        // lightning-address branch and is rejected for malformed shape.
        assert!(lnurl_to_well_known_url(&encoded).is_err());
    }

    // Empty + nonsense.
    #[test]
    fn empty_input_is_rejected() {
        assert!(lnurl_to_well_known_url("").is_err());
        assert!(lnurl_to_well_known_url("   ").is_err());
    }

    #[test]
    fn nonsense_input_is_rejected() {
        assert!(lnurl_to_well_known_url("not a url or address").is_err());
    }

    // Bolt11 sniff.
    #[test]
    fn bolt11_sniff_accepts_known_hrps() {
        assert!(looks_like_bolt11("lnbc100n1p…"));
        assert!(looks_like_bolt11("LNBC100N1P…")); // case-insensitive
        assert!(looks_like_bolt11("lntb100n1p…"));
        assert!(looks_like_bolt11("lnbcrt100n1p…"));
        assert!(looks_like_bolt11("lntbs100n1p…"));
    }

    #[test]
    fn bolt11_sniff_rejects_html_or_garbage() {
        assert!(!looks_like_bolt11("<html>"));
        assert!(!looks_like_bolt11(""));
        assert!(!looks_like_bolt11("not an invoice"));
    }

    // URL encode.
    #[test]
    fn url_encode_passes_unreserved_chars() {
        assert_eq!(
            url_encode_query("abcXYZ012-._~"),
            "abcXYZ012-._~"
        );
    }

    #[test]
    fn url_encode_escapes_reserved_chars() {
        // `=`, `&`, `?`, `/`, ` `, `"`, `{`, `}` must all be percent-encoded
        // so they don't break the query-string boundaries or JSON shape.
        assert_eq!(url_encode_query("a=b&c"), "a%3Db%26c");
        assert_eq!(url_encode_query("{\"k\":\"v\"}"), "%7B%22k%22%3A%22v%22%7D");
        assert_eq!(url_encode_query(" "), "%20");
    }

    #[test]
    fn url_encode_handles_utf8_multibyte() {
        // 🤙 = U+1F919 = F0 9F A4 99 in UTF-8.
        assert_eq!(url_encode_query("🤙"), "%F0%9F%A4%99");
    }
}
