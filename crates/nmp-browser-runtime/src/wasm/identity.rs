// Identity-validation helpers are fully used on wasm32; on native they are
// exercised only from `#[cfg(test)]` blocks in this file and `core.rs`.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

//! Active-identity validation for `WorkerRequest::SetIdentity` (#2038 item A).
//!
//! Mirrors `nmp-wasm/src/signer_slot.rs` but defined here so
//! `nmp-browser-runtime` stays free of a dep on the ABI crate. The validation
//! logic is identical: `kind = "nip07"` is the only wired signer kind; the
//! pubkey hex is parsed by the `nostr` crate's `PublicKey` to canonicalize it
//! to lowercase (B2 – uppercase input must not reach the kernel as-is).
//!
//! Always-compiled: `NmpRuntimeCore::handle_set_identity` needs this on native
//! CI too.

/// Possible failures when validating a `SetIdentity` request.
#[derive(Debug)]
pub(crate) enum IdentityError {
    /// The host supplied a signer kind the wasm runtime does not yet wire.
    UnsupportedKind(String),
    /// The pubkey hex did not parse as a valid 64-char hex pubkey.
    InvalidPubkey(String),
}

impl IdentityError {
    /// Human-readable detail string suitable for `CapabilityFailure.reason`.
    /// Always starts with the stable code so hosts can split on `": "`.
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::UnsupportedKind(kind) => format!(
                "unsupported_signer_kind: \"{kind}\" — only \"nip07\" is wired. \
                 NIP-46 bunker signing is deferred to #2119/#2068."
            ),
            Self::InvalidPubkey(detail) => format!("invalid_signer_pubkey: {detail}"),
        }
    }
}

/// Return `true` iff `value` is exactly 64 lowercase-or-uppercase ASCII hex digits.
///
/// Mirrors `nmp_core::kernel::nostr::is_hex_pubkey` without importing it (that
/// function is not re-exported at the `nmp_core` crate root).
fn is_valid_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Validate and canonicalize the pubkey hex from a `SetIdentity` request.
///
/// Returns the lowercase canonical hex on success. Pure — no I/O.
///
/// Validation: checks that the value is exactly 64 lowercase-ASCII hex digits
/// (via `nmp_core::is_hex_pubkey`, which accepts uppercase too — any uppercase
/// input is normalized to lowercase, satisfying B2). Full secp256k1 point
/// validity is deferred to `KernelReducer::set_active_account`; the gate here
/// is a cheap format pre-flight.
pub(crate) fn canonical_pubkey_from_kind(
    kind: &str,
    pubkey_hex: &str,
) -> Result<String, IdentityError> {
    match kind {
        "nip07" => {
            // Format pre-flight: must be exactly 64 ASCII hex digits (upper or lower).
            // Full secp256k1 point validity is deferred to KernelReducer::set_active_account.
            if !is_valid_hex_pubkey(pubkey_hex) {
                return Err(IdentityError::InvalidPubkey(format!(
                    "pubkey_hex {:?} is not a valid 64-char hex string",
                    pubkey_hex
                )));
            }
            // Canonicalize: lowercase (B2 — uppercase input must not reach kernel as-is).
            Ok(pubkey_hex.to_ascii_lowercase())
        }
        other => Err(IdentityError::UnsupportedKind(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    #[test]
    fn nip07_valid_hex_returns_lowercase_canonical() {
        let result = canonical_pubkey_from_kind("nip07", PK).expect("must succeed");
        assert_eq!(result, PK);
    }

    #[test]
    fn nip07_uppercase_is_normalized_to_lowercase() {
        let upper = PK.to_ascii_uppercase();
        let result = canonical_pubkey_from_kind("nip07", &upper).expect("must succeed");
        assert_eq!(
            result, PK,
            "uppercase input must be normalized to lowercase (B2)"
        );
    }

    #[test]
    fn unknown_kind_returns_unsupported_error() {
        let err = canonical_pubkey_from_kind("magic", PK).expect_err("must fail");
        assert!(err.detail().contains("unsupported_signer_kind"));
        assert!(err.detail().contains("magic"));
    }

    #[test]
    fn garbage_hex_returns_invalid_pubkey_error() {
        let err = canonical_pubkey_from_kind("nip07", "not-valid-hex").expect_err("must fail");
        assert!(err.detail().contains("invalid_signer_pubkey"));
    }
}
