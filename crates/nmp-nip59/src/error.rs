//! Error type for NIP-59 operations.

use std::fmt;

/// Errors that can occur during NIP-59 gift-wrap and unwrap operations.
#[derive(Debug, PartialEq)]
pub enum Nip59Error {
    /// The event is not a gift-wrap (kind:1059).
    NotGiftWrap,
    /// The rumor author does not match the seal signer (spoofing attempt).
    SenderMismatch,
    /// Cryptographic or serialisation failure from the underlying nostr library.
    Nostr(String),
}

impl fmt::Display for Nip59Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotGiftWrap => f.write_str("not a gift-wrap event (expected kind 1059)"),
            Self::SenderMismatch => f.write_str("sender public key mismatch in seal/rumor"),
            Self::Nostr(msg) => write!(f, "nostr error: {msg}"),
        }
    }
}

impl std::error::Error for Nip59Error {}

impl From<nostr::nips::nip59::Error> for Nip59Error {
    fn from(e: nostr::nips::nip59::Error) -> Self {
        match e {
            nostr::nips::nip59::Error::NotGiftWrap => Self::NotGiftWrap,
            nostr::nips::nip59::Error::SenderMismatch => Self::SenderMismatch,
            other => Self::Nostr(other.to_string()),
        }
    }
}

impl From<nostr::event::Error> for Nip59Error {
    fn from(e: nostr::event::Error) -> Self {
        Self::Nostr(e.to_string())
    }
}

impl From<nostr::event::builder::Error> for Nip59Error {
    fn from(e: nostr::event::builder::Error) -> Self {
        Self::Nostr(e.to_string())
    }
}
