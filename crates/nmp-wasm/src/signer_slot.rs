//! V-01 Stage 3b — signer slot for app-level writes.
//!
//! `WasmRuntime` carries `Option<Arc<dyn Signer>>` populated via
//! [`WorkerRequest::SetSigner`]. Until the slot is filled, every app-level
//! write (PublishNote / React / Follow / Unfollow) honestly returns
//! `signer_not_installed`. With the slot filled, the writes still return a
//! `publish_path_not_wired` capability failure — Stage 3b lands the *signer*
//! plumbing; Stage 3c will expose the `KernelReducer` publish-from-signed-
//! event surface and route through `PublishEngine` from wasm.
//!
//! # Why a separate file
//!
//! Keeps `runtime.rs` under the 500-line ceiling and concentrates the
//! kind→constructor mapping in one place — when bunker (NIP-46) gets a wasm
//! transport, the additional arm lands here, not as another branch inside
//! the runtime's request dispatcher.

use std::sync::Arc;

use nmp_signers::{Nip07Signer, Signer};
use nostr::PublicKey;

use crate::protocol::SetSigner;

/// Outcome of attempting to construct + install a signer from a
/// [`SetSigner`] request. `Debug` is derived so test assertions (and any
/// future log/trace plumbing) can render the variant without manual
/// formatting; the variants themselves carry no key material so the derive
/// is leak-free.
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
                "unsupported_signer_kind: \"{kind}\" — only \"nip07\" is wired \
                 in V-01 Stage 3b. NIP-46 bunker on wasm requires a wasm \
                 NIP-46 transport (Stage 3c follow-up); LocalKey signers \
                 require key material the wasm runtime should not hold."
            ),
            Self::InvalidPubkey(detail) => format!("invalid_signer_pubkey: {detail}"),
        }
    }
}

/// Construct a [`Signer`] from a [`SetSigner`] request. Pure: no I/O, no
/// thread-spawning, no JS-event-loop interaction. The actual signing path is
/// inside the returned `Arc<dyn Signer>`.
///
/// `nip07` is the only kind wired; other kinds are rejected so the host has
/// an honest, stable error to surface to the user.
pub(crate) fn install_from_request(
    request: &SetSigner,
) -> Result<Arc<dyn Signer>, SignerInstallError> {
    match request.kind.as_str() {
        "nip07" => {
            let pubkey = PublicKey::from_hex(&request.pubkey_hex).map_err(|e| {
                SignerInstallError::InvalidPubkey(format!(
                    "could not parse pubkey_hex {:?}: {e}",
                    request.pubkey_hex
                ))
            })?;
            Ok(Arc::new(Nip07Signer::from_cached_pubkey(pubkey)))
        }
        other => Err(SignerInstallError::UnsupportedKind(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_nip07_with_valid_hex_succeeds() {
        let request = SetSigner {
            kind: "nip07".to_string(),
            pubkey_hex:
                "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
                    .to_string(),
            correlation_id: "set-1".to_string(),
        };
        let signer = install_from_request(&request).expect("install must succeed");
        // Round-trip through the trait — proves we wired Nip07Signer, not
        // some other backend.
        assert_eq!(
            signer.backend(),
            nmp_signers::SignerBackend::Nip07,
            "must install the NIP-07 backend for kind = \"nip07\""
        );
    }

    #[test]
    fn install_unknown_kind_returns_unsupported() {
        let request = SetSigner {
            kind: "magic".to_string(),
            pubkey_hex: String::new(),
            correlation_id: "set-1".to_string(),
        };
        let error = install_from_request(&request).expect_err("must fail");
        assert_eq!(error.code(), "unsupported_signer_kind");
        assert!(error.detail().contains("magic"));
    }

    #[test]
    fn install_nip07_with_garbage_hex_returns_invalid_pubkey() {
        let request = SetSigner {
            kind: "nip07".to_string(),
            pubkey_hex: "not-hex".to_string(),
            correlation_id: "set-1".to_string(),
        };
        let error = install_from_request(&request).expect_err("must fail");
        assert_eq!(error.code(), "invalid_signer_pubkey");
    }
}
