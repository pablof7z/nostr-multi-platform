//! Signing value types shared across the publish / signer pipeline.
//!
//! The signing value types below (`UnsignedEvent`, `SignedEvent`, `SigningError`)
//! are load-bearing: the publish engine, the NIP-42 flow, and every signer crate
//! exchange events through them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnsignedEvent {
    pub pubkey: String,
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SignedEvent {
    pub id: String,
    pub sig: String,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SigningError {
    Unsupported(String),
    Rejected(String),
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
