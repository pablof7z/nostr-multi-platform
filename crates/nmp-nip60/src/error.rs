//! Unified error type for nmp-nip60.

use std::fmt;

#[derive(Debug)]
pub enum Nip60Error {
    /// HTTP call to a Cashu mint failed.
    MintHttp(String),
    /// Cashu mint returned a protocol error.
    MintProtocol(String),
    /// Blind signature or DLEQ verification failed.
    Crypto(String),
    /// Nostr event encode/decode error.
    Event(String),
    /// NIP-44 encryption/decryption error.
    Nip44(String),
    /// JSON parse error.
    Json(serde_json::Error),
    /// Relay-layer error surfaced by the kernel's publish/ingest pipeline.
    Relay(String),
    /// Wallet is not initialised (no mints configured).
    NotInitialised,
    /// Insufficient balance to complete the operation.
    InsufficientBalance { have: u64, need: u64 },
    /// P2PK spending condition error.
    SpendingCondition(String),
    /// NIP-87 mint discovery failed.
    MintDiscovery(String),
    /// General validation error.
    Invalid(String),
    /// Mint quote has not been paid yet — caller should retry later.
    QuoteNotPaid,
    /// Nutzap receipt was already redeemed.
    AlreadyRedeemed(nostr::EventId),
}

impl fmt::Display for Nip60Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MintHttp(e) => write!(f, "mint HTTP error: {e}"),
            Self::MintProtocol(e) => write!(f, "mint protocol error: {e}"),
            Self::Crypto(e) => write!(f, "cashu crypto error: {e}"),
            Self::Event(e) => write!(f, "nostr event error: {e}"),
            Self::Nip44(e) => write!(f, "NIP-44 error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Relay(e) => write!(f, "relay error: {e}"),
            Self::NotInitialised => write!(f, "wallet not initialised (no mints configured)"),
            Self::InsufficientBalance { have, need } => {
                write!(f, "insufficient balance: have {have} sat, need {need} sat")
            }
            Self::SpendingCondition(e) => write!(f, "P2PK spending condition: {e}"),
            Self::MintDiscovery(e) => write!(f, "mint discovery: {e}"),
            Self::Invalid(e) => write!(f, "invalid: {e}"),
            Self::QuoteNotPaid => write!(f, "mint quote not yet paid — retry after a moment"),
            Self::AlreadyRedeemed(event_id) => write!(f, "nutzap already redeemed: {event_id}"),
        }
    }
}

impl std::error::Error for Nip60Error {}

impl From<serde_json::Error> for Nip60Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
