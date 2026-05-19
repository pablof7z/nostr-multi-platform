//! # nmp-signers
//!
//! Signer trait + concrete implementations + multi-account `AccountManager`
//! for the NMP framework.  Sits outside `nmp-core` per doctrine **D0** —
//! identity/signer materials are policy + capability bridges, not kernel
//! substrate.
//!
//! ## Layout
//!
//! - `signers::Signer` — minimal trait (sync `pubkey`, async-via-thunk `sign`).
//! - `signers::local::LocalKeySigner` — in-memory nsec (optional NIP-49 at rest).
//! - `signers::nip46::Nip46Signer` — bunker:// remote signer scaffolding.
//! - `signers::nip07::Nip07Signer` — wasm browser extension (stub off-wasm).
//! - `bunker::parse_bunker_uri` — canonical NIP-46 URL parser (fuzz target).
//! - `identity::AccountManager` — multi-account runtime state with synchronous
//!   active-switch guarantees + applesauce-style mismatch post-conditions.
//!
//! See `docs/decisions/0015-m6-signer-design.md` for the design rationale.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod bunker;
pub mod identity;
pub mod signers;

pub use bunker::{parse_bunker_uri, BunkerParseError, BunkerUri, MAX_BUNKER_URI_LEN};
pub use identity::{
    bundle_for, AccountError, AccountManager, ActiveAccountReactor, ActiveChangeEvent,
    ActiveChangeObserver, ActiveSwitch, ActiveSwitchCommand, IdentityId, Kind3RewireEvent,
    Kind3RewireObserver,
};
pub use signers::{
    LocalKeySigner, Nip04, Nip44, Nip46Rpc, Nip46Signer, Nip46SignerHandle, Nip46Transport,
    Nip07Signer, Signer, SignerBackend, SignerError, SignerOp, SignerPayload,
};

pub use nostr::{PublicKey, SecretKey};
