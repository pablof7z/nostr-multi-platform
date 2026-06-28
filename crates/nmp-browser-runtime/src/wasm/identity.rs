// Identity-validation helpers are fully used on wasm32; on native they are
// exercised only from `#[cfg(test)]` blocks in this file and `core.rs`.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

//! Active-identity validation for `WorkerRequest::SetIdentity` (#2038 item A).
//!
//! Mirrors `nmp-wasm/src/signer_slot.rs` but defined here so
//! `nmp-browser-runtime` stays free of a dep on the ABI crate. The validation
//! logic keeps NIP-07 pubkey canonicalization and browser local-key signer
//! installation in Rust. TypeScript never decodes or signs with a pasted nsec.
//!
//! Always-compiled: `NmpRuntimeCore::handle_set_identity` needs this on native
//! CI too.

use std::sync::Arc;

use nmp_core::OutboundMessage;
use nmp_signers::{LocalKeySigner, Signer};
use zeroize::Zeroize;

use crate::runtime::BrowserRuntimeHandle;

use super::protocol::SetIdentity;

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
                "unsupported_signer_kind: \"{kind}\" — supported signer kinds are \
                 \"nip07\", \"local_key\", and \"nip46\"."
            ),
            Self::InvalidPubkey(detail) => format!("invalid_signer_pubkey: {detail}"),
        }
    }
}

pub(crate) enum IdentityInstallOutcome {
    ActiveAccount(String),
    PendingBunker(Vec<OutboundMessage>),
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

pub(crate) fn install_identity(
    handle: &mut BrowserRuntimeHandle,
    req: &mut SetIdentity,
) -> Result<IdentityInstallOutcome, String> {
    match req.kind.as_str() {
        "nip07" => canonical_pubkey_from_kind(&req.kind, &req.pubkey_hex)
            .map(IdentityInstallOutcome::ActiveAccount)
            .map_err(|err| err.detail()),
        "local_key" => {
            let Some(mut secret) = req
                .secret_key_bech32
                .take()
                .filter(|value| !value.is_empty())
            else {
                return Err(
                    "missing_local_key: secret_key_bech32 is required for local_key".to_string(),
                );
            };
            let signer = LocalKeySigner::from_nsec(&secret)
                .map_err(|err| format!("invalid_local_key: {err}"));
            secret.zeroize();
            let signer = signer?;
            Ok(IdentityInstallOutcome::ActiveAccount(
                handle.install_signer_provider(Arc::new(signer) as Arc<dyn Signer>),
            ))
        }
        "nip46" => {
            let Some(mut bunker_uri) = req.bunker_uri.take().filter(|value| !value.is_empty())
            else {
                return Err(
                    "missing_nip46_bunker_uri: bunker_uri is required for nip46".to_string()
                );
            };
            let outbound = handle.begin_nip46_bunker(&bunker_uri);
            bunker_uri.zeroize();
            outbound.map(IdentityInstallOutcome::PendingBunker)
        }
        other => Err(format!(
            "unsupported_signer_kind: \"{other}\" — supported signer kinds are \"nip07\", \
             \"local_key\", and \"nip46\"."
        )),
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
