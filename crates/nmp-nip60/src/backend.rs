//! `WalletBackend` — the unified wallet abstraction.
//!
//! Apps never need to know whether the user's wallet is a NIP-60 ecash
//! wallet or a NWC-connected Lightning wallet. Both implement `WalletBackend`
//! and are accessed through the same interface.
//!
//! # Design
//!
//! - The trait is `Send + Sync` so it can be stored in an `Arc<dyn WalletBackend>`.
//! - Operations are blocking — call from a worker thread (D8).
//! - `pay_invoice` works for both NIP-60 (melt) and NWC (kind:23194 request).
//! - `create_nutzap_proofs` is NIP-60–specific; NWC returns `Err(Unsupported)`.
//! - `balance_sats` returns the current wallet balance.

use crate::error::Nip60Error;
use crate::nutzap::NutZapProof;

/// Result of a lightning payment.
#[derive(Debug, Clone)]
pub struct PayResult {
    /// Pre-image of the paid invoice (hex). May be empty for ecash swaps.
    pub preimage: Option<String>,
    /// Actual fee paid in millisats (if known).
    pub fee_msats: Option<u64>,
}

/// Errors that can surface from a `WalletBackend` operation.
#[derive(Debug)]
pub enum WalletError {
    /// The underlying NIP-60 error.
    Nip60(Nip60Error),
    /// This operation is not supported by the backend type.
    Unsupported,
    /// Payment failed with a reason string.
    PaymentFailed(String),
    /// General error.
    Other(String),
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nip60(e) => write!(f, "nip60: {e}"),
            Self::Unsupported => write!(f, "operation not supported by this wallet backend"),
            Self::PaymentFailed(r) => write!(f, "payment failed: {r}"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for WalletError {}

impl From<Nip60Error> for WalletError {
    fn from(e: Nip60Error) -> Self {
        Self::Nip60(e)
    }
}

/// The unified wallet interface — transparent to callers whether NIP-60 or NWC.
pub trait WalletBackend: Send + Sync {
    /// Current balance in satoshis. Returns 0 if unknown.
    fn balance_sats(&self) -> u64;

    /// Pay a Lightning bolt11 invoice.
    ///
    /// - For NIP-60: melts ecash tokens at the mint.
    /// - For NWC: sends a `pay_invoice` request to the connected wallet.
    fn pay_invoice(&self, bolt11: &str) -> Result<PayResult, WalletError>;

    /// Create P2PK-locked proofs suitable for a NutZap (NIP-61).
    ///
    /// Returns an error with `WalletError::Unsupported` for non-NIP-60 backends.
    fn create_nutzap_proofs(
        &self,
        amount_sats: u64,
        recipient_cashu_pubkey: &str,
        mint_url: &str,
    ) -> Result<Vec<NutZapProof>, WalletError>;

    /// Type name for diagnostics.
    fn backend_type(&self) -> &'static str;
}
