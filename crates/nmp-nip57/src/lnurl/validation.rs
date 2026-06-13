//! Pre-pay BOLT-11 invoice validation — the fail-closed guards that run on a
//! fetched LNURL invoice **before** it is handed to the wallet for automatic
//! payment. Two checks live here:
//!
//! 1. [`validate_bolt11_amount`] — the encoded amount must equal exactly what
//!    the user requested (no amountless invoices, no over/undercharge).
//! 2. [`validate_description_hash`] — in NIP-57 zap mode the invoice's
//!    BOLT-11 `description_hash` (`h` field) must equal `SHA-256` of the exact
//!    serialized signed zap request sent as the LNURL `nostr=` parameter, so a
//!    hostile or buggy provider cannot swap in an invoice for an unrelated
//!    payment.
//!
//! Both live here (not in `lnurl/mod.rs`) because that file is at its
//! size-budget ceiling and pre-pay validation is a self-contained concern.

/// Validate that a bolt11 invoice encodes exactly `requested_msats`.
///
/// Parses the BOLT-11 HRP with [`crate::bolt11::amount_msats`] and compares the
/// result against the user-chosen amount. Returns `Err` when the invoice is
/// **amountless** (parser returns `None`) — fail closed, because an
/// unverifiable invoice must not be auto-paid — or when the encoded amount
/// **does not match** the requested amount (a buggy or hostile provider; the
/// error names both values). Returns `Ok(())` only on an exact match.
pub(crate) fn validate_bolt11_amount(bolt11: &str, requested_msats: u64) -> Result<(), String> {
    match crate::bolt11::amount_msats(bolt11) {
        None => Err(format!(
            "LNURL provider returned an amountless bolt11 invoice — \
             refusing automatic payment of an unverifiable amount \
             (requested {requested_msats} msats)"
        )),
        Some(actual) if actual != requested_msats => Err(format!(
            "LNURL provider invoice amount mismatch: requested {requested_msats} msats \
             but bolt11 encodes {actual} msats — refusing automatic payment"
        )),
        Some(_) => Ok(()),
    }
}

/// Validate the NIP-57 description-hash commitment.
///
/// `nostr_mode` is `true` when the endpoint advertised a non-empty
/// `nostrPubkey` — the signal that it operates as a NIP-57 zap endpoint. The
/// check is only enforced in that mode; a plain (non-zap) LNURL-pay invoice has
/// no obligation to commit to a zap request, so this is a no-op there.
///
/// Fail-closed semantics (D6 — errors as state, never panic): when in zap mode,
/// the invoice is rejected if it carries **no** description hash *or* if the
/// hash does not match. The caller runs this before handing the bolt11 to the
/// wallet, so a mismatch can never reach an automatic payment.
pub(crate) fn validate_description_hash(
    bolt11: &str,
    signed_zap_request_json: &str,
    nostr_mode: bool,
) -> Result<(), String> {
    if !nostr_mode {
        return Ok(());
    }
    let Some(invoice_hash) = crate::bolt11::description_hash(bolt11) else {
        return Err(
            "LNURL provider runs in NIP-57 zap mode (advertised `nostrPubkey`) but the bolt11 \
             invoice carries no description hash — refusing automatic payment of an invoice that \
             does not commit to the zap request"
                .to_string(),
        );
    };
    let expected = sha256_of(signed_zap_request_json.as_bytes());
    if invoice_hash != expected {
        return Err(format!(
            "NIP-57 description-hash mismatch: bolt11 commits to {} but the signed zap request \
             hashes to {} — refusing automatic payment of an invoice bound to a different request",
            hex_lower(&invoice_hash),
            hex_lower(&expected),
        ));
    }
    Ok(())
}

/// SHA-256 of `bytes` via the audited `sha2` crate (no hand-rolled crypto).
fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Lowercase-hex render of a 32-byte hash for diagnostic error messages.
fn hex_lower(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use bech32::primitives::iter::Fe32IterExt;
    use bech32::{Fe32, Hrp};

    /// BOLT-11 tagged-field type for the description hash (`h`).
    const TAG_DESCRIPTION_HASH: u8 = 23;
    /// Timestamp prefix groups in the BOLT-11 data part.
    const TIMESTAMP_FE32_GROUPS: usize = 7;

    /// Pack a 32-byte hash into the 52 five-bit BOLT-11 groups (mirrors the
    /// production decoder in `crate::bolt11`).
    fn hash_to_fe32_groups(hash: &[u8; 32]) -> Vec<Fe32> {
        let mut groups = Vec::with_capacity(52);
        let mut acc: u32 = 0;
        let mut bits: u32 = 0;
        for &b in hash {
            acc = (acc << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                groups.push(Fe32::try_from(((acc >> bits) & 0x1f) as u8).unwrap());
            }
        }
        if bits > 0 {
            groups.push(Fe32::try_from(((acc << (5 - bits)) & 0x1f) as u8).unwrap());
        }
        groups
    }

    /// Build a synthetic `lnbc…` invoice whose only tagged field is an `h`
    /// (description hash) field carrying `hash`.
    fn synth_invoice_with_h(hash: &[u8; 32]) -> String {
        let zero = Fe32::try_from(0u8).unwrap();
        let mut data: Vec<Fe32> = Vec::new();
        data.extend(std::iter::repeat_n(zero, TIMESTAMP_FE32_GROUPS));
        data.push(Fe32::try_from(TAG_DESCRIPTION_HASH).unwrap());
        data.push(Fe32::try_from(1u8).unwrap()); // length hi  (1<<5)
        data.push(Fe32::try_from(20u8).unwrap()); // length lo  → 52
        data.extend(hash_to_fe32_groups(hash));
        let hrp = Hrp::parse("lnbc").unwrap();
        data.into_iter()
            .with_checksum::<bech32::Bech32>(&hrp)
            .chars()
            .collect()
    }

    /// Build an invoice whose `h` field commits to `SHA-256(zap_request)` —
    /// the well-formed NIP-57 case.
    fn invoice_committing_to(zap_request_json: &str) -> String {
        let hash = sha256_of(zap_request_json.as_bytes());
        synth_invoice_with_h(&hash)
    }

    const ZAP_REQUEST: &str =
        r#"{"kind":9734,"content":"","tags":[["relays","wss://relay.example"]]}"#;

    #[test]
    fn passes_when_hash_matches_in_nostr_mode() {
        let invoice = invoice_committing_to(ZAP_REQUEST);
        assert_eq!(
            validate_description_hash(&invoice, ZAP_REQUEST, true),
            Ok(()),
            "an invoice committing to SHA-256(zap request) must validate"
        );
    }

    #[test]
    fn fails_when_hash_commits_to_a_different_request() {
        // The invoice commits to a *different* zap request than the one we sent.
        let invoice = invoice_committing_to(r#"{"kind":9734,"content":"OTHER"}"#);
        let result = validate_description_hash(&invoice, ZAP_REQUEST, true);
        assert!(
            result.is_err(),
            "a mismatched commitment must be rejected: {result:?}"
        );
        assert!(
            result.unwrap_err().contains("description-hash mismatch"),
            "error must name the mismatch"
        );
    }

    #[test]
    fn fails_when_invoice_has_no_description_hash_in_nostr_mode() {
        // Amount-only invoice, no `h` field — must fail closed in zap mode.
        let result = validate_description_hash("lnbc2500u1pvjluez000", ZAP_REQUEST, true);
        assert!(
            result.is_err(),
            "a zap-mode invoice with no description hash must be rejected: {result:?}"
        );
        assert!(result.unwrap_err().contains("no description hash"));
    }

    #[test]
    fn no_op_when_not_in_nostr_mode() {
        // Outside zap mode (no nostrPubkey), even a hashless invoice is fine —
        // the commitment obligation does not apply.
        assert_eq!(
            validate_description_hash("lnbc2500u1pvjluez000", ZAP_REQUEST, false),
            Ok(()),
            "non-zap LNURL-pay invoices carry no commitment obligation"
        );
    }

    #[test]
    fn matched_hash_is_independent_of_invoice_amount() {
        // The commitment is to the request, not the amount; an arbitrary HRP
        // amount in the synthetic invoice does not affect the hash check.
        let invoice = invoice_committing_to(ZAP_REQUEST);
        assert!(validate_description_hash(&invoice, ZAP_REQUEST, true).is_ok());
        // A single-byte change in the request flips the verdict.
        let tampered = format!("{ZAP_REQUEST} ");
        assert!(
            validate_description_hash(&invoice, &tampered, true).is_err(),
            "any change to the request bytes must break the commitment"
        );
    }
}
