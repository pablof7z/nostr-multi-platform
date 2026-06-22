//! Active-identity validation for [`WorkerRequest::SetSigner`].
//!
//! ADR-0064 §5: the wasm runtime no longer installs a persistent
//! `Arc<dyn Signer>`. A `SetSigner` request only carries the user's public
//! identity; this module validates + canonicalizes the supplied pubkey (the
//! backend `kind` is honestly gated) so the runtime can seed the kernel's
//! active account. Signing is the ADR-0050 capability round-trip
//! (`BeginSign` → `SignRequest` → `DeliverSignerResponse`), not a slot.
//!
//! # Why a separate file
//!
//! Keeps `runtime.rs` under the 500-line ceiling and concentrates the
//! kind-gating + pubkey-canonicalization in one place — when bunker (NIP-46)
//! gets a wasm capability fulfiller, the additional kind lands here, not as
//! another branch inside the runtime's request dispatcher.

use nostr::PublicKey;

use crate::protocol::SetSigner;

/// Outcome of validating a [`SetSigner`] identity request. `Debug` is derived
/// so test assertions (and any future log/trace plumbing) can render the
/// variant without manual formatting; the variants carry no key material so
/// the derive is leak-free.
#[derive(Debug)]
pub(crate) enum SignerInstallError {
    /// The host asked for a signer kind the wasm runtime does not yet wire.
    UnsupportedKind(String),
    /// The supplied pubkey hex did not parse as a valid secp256k1 x-only
    /// public key.
    InvalidPubkey(String),
}

impl SignerInstallError {
    /// Stable error code the JS host can pattern-match on. Currently only
    /// referenced from the in-crate tests (the runtime embeds the same
    /// prefix at the start of [`Self::detail`] so external callers can do
    /// the same split without an extra accessor); kept on the surface so
    /// future callers don't have to re-derive the mapping from the
    /// detail-string format.
    #[allow(dead_code)]
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedKind(_) => "unsupported_signer_kind",
            Self::InvalidPubkey(_) => "invalid_signer_pubkey",
        }
    }

    /// Human-readable detail, suitable for inclusion in a
    /// `CapabilityFailure.reason`. Always starts with the stable
    /// [`Self::code`] so hosts can split on the first `: `.
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::UnsupportedKind(kind) => format!(
                "unsupported_signer_kind: \"{kind}\" — only \"nip07\" is wired. \
                 NIP-46 bunker / NIP-55 Android on wasm join as ADR-0050 sign \
                 capability fulfillers; LocalKey signers require key material \
                 the wasm runtime should not hold."
            ),
            Self::InvalidPubkey(detail) => format!("invalid_signer_pubkey: {detail}"),
        }
    }
}

/// Validate + canonicalize the active-account pubkey from a [`SetSigner`]
/// identity request. Pure: no I/O, no thread-spawning, no JS-event-loop
/// interaction, and (ADR-0064 §5) no persistent signer construction.
///
/// Returns the lowercase canonical hex of the pubkey — derived from the parsed
/// key so any uppercase input is normalised before the kernel stores it.
///
/// `nip07` is the only kind wired; other kinds are rejected so the host has
/// an honest, stable error to surface to the user.
pub(crate) fn canonical_pubkey_from_request(
    request: &SetSigner,
) -> Result<String, SignerInstallError> {
    match request.kind.as_str() {
        "nip07" => {
            let pubkey = PublicKey::from_hex(&request.pubkey_hex).map_err(|e| {
                SignerInstallError::InvalidPubkey(format!(
                    "could not parse pubkey_hex {:?}: {e}",
                    request.pubkey_hex
                ))
            })?;
            Ok(pubkey.to_hex())
        }
        other => Err(SignerInstallError::UnsupportedKind(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nip07_with_valid_hex_canonicalizes() {
        let request = SetSigner {
            kind: "nip07".to_string(),
            pubkey_hex:
                "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
                    .to_string(),
            correlation_id: "set-1".to_string(),
        };
        let canonical_hex =
            canonical_pubkey_from_request(&request).expect("validation must succeed");
        // Canonical hex must be lowercase regardless of input case.
        assert_eq!(
            canonical_hex,
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
            "canonical_hex must be lowercase"
        );
    }

    #[test]
    fn unknown_kind_returns_unsupported() {
        let request = SetSigner {
            kind: "magic".to_string(),
            pubkey_hex: String::new(),
            correlation_id: "set-1".to_string(),
        };
        let error = canonical_pubkey_from_request(&request).expect_err("must fail");
        assert_eq!(error.code(), "unsupported_signer_kind");
        assert!(error.detail().contains("magic"));
    }

    #[test]
    fn nip07_with_garbage_hex_returns_invalid_pubkey() {
        let request = SetSigner {
            kind: "nip07".to_string(),
            pubkey_hex: "not-hex".to_string(),
            correlation_id: "set-1".to_string(),
        };
        let error = canonical_pubkey_from_request(&request).expect_err("must fail");
        assert_eq!(error.code(), "invalid_signer_pubkey");
    }

    #[test]
    fn nip07_uppercase_hex_returns_canonical_lowercase() {
        // B2 canonicalization guard: an uppercase pubkey must be normalised to
        // lowercase so `set_active_account` stores a canonical key.
        let request = SetSigner {
            kind: "nip07".to_string(),
            pubkey_hex:
                "3BF0C63FCB93463407AF97A5E5EE64FA883D107EF9E558472C4EB9AAAEFA459D"
                    .to_string(),
            correlation_id: "set-1".to_string(),
        };
        let canonical_hex = canonical_pubkey_from_request(&request).expect("must succeed");
        assert_eq!(
            canonical_hex,
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
            "canonical_hex must be lowercase even when input is uppercase"
        );
    }
}
