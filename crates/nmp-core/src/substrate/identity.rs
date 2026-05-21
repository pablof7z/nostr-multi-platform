//! Signing value types shared across the publish / signer pipeline.
//!
//! NOTE: this module once also defined an `IdentityModule` substrate trait
//! (plus `IdentityContext` / `IdentityScopeKind` / `IdentityError` / `IdentityId`
//! / `BoxFuture`). That trait was a v2 extension contract the kernel never
//! drove — no registry stored `dyn IdentityModule`, and nothing implemented it.
//! It has been removed. The signing value types below (`UnsignedEvent`,
//! `SignedEvent`, `SigningError`) are load-bearing: the publish engine, the
//! NIP-42 flow, and every signer crate exchange events through them.

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SigningError {
    Unsupported(String),
    Rejected(String),
    Failed(String),
}
