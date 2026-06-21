//! Signing value types shared across the publish / signer pipeline.
//!
//! The signing value types below (`UnsignedEvent`, `SignedEvent`, `SigningError`)
//! are load-bearing: the publish engine, the NIP-42 flow, and every signer crate
//! exchange events through them. They are dependency-light vocabulary (serde
//! value types only, no kernel behavior) and therefore live in this tier-0
//! interface crate rather than in `nmp-core`. `nmp-core` re-exports them through
//! `nmp_core::substrate` so existing kernel-side and protocol-crate import paths
//! are unchanged.

use serde::{Deserialize, Serialize};

/// Unsigned NIP-01 event template handed to a signer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnsignedEvent {
    /// Author pubkey (lowercase hex).
    pub pubkey: String,
    /// Event kind.
    pub kind: u32,
    /// Tag rows.
    pub tags: Vec<Vec<String>>,
    /// Event content.
    pub content: String,
    /// Creation timestamp (unix seconds).
    pub created_at: u64,
}

/// A signed NIP-01 event: the `UnsignedEvent` plus its id and signature.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SignedEvent {
    /// Event id (lowercase hex).
    pub id: String,
    /// Schnorr signature (lowercase hex).
    pub sig: String,
    /// The signed template.
    pub unsigned: UnsignedEvent,
}

impl SignedEvent {
    /// Serialize to the FLAT NIP-01 wire JSON object
    /// (`{ id, pubkey, created_at, kind, tags, content, sig }`), NOT this
    /// type's nested `derive(Serialize)` shape (which nests under `unsigned`).
    ///
    /// This is the form every relay and out-of-band transport (e.g. a Blossom
    /// `Authorization: Nostr <base64(json)>` header) expects. Generic — no
    /// protocol noun; the actor's sign-and-return drain and protocol-crate
    /// workers share this one serializer.
    #[must_use]
    pub fn to_nip01_json(&self) -> String {
        serde_json::json!({
            "id": self.id,
            "pubkey": self.unsigned.pubkey,
            "created_at": self.unsigned.created_at,
            "kind": self.unsigned.kind,
            "tags": self.unsigned.tags,
            "content": self.unsigned.content,
            "sig": self.sig,
        })
        .to_string()
    }
}

/// Error returned by a synchronous signing attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SigningError {
    /// The signer does not support the requested operation.
    Unsupported(String),
    /// The user (or backend policy) rejected the request.
    Rejected(String),
    /// The signing attempt failed for another reason.
    Failed(String),
}

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(msg) => write!(f, "signing unsupported: {msg}"),
            Self::Rejected(msg) => write!(f, "signing rejected: {msg}"),
            Self::Failed(msg) => write!(f, "signing failed: {msg}"),
        }
    }
}

impl std::error::Error for SigningError {}
