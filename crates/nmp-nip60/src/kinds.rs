//! Kind constants for NIP-60 (Cashu wallet) and related NIPs.
//!
//! The kind *integers* are the workspace canon in `nmp-kinds` (the zero-dep
//! Layer-0 registry). This module re-exports them under the names this crate's
//! builders/decoders already use, so there is exactly one definition of each
//! integer. They are `u32` (the registry's uniform width); the `EventBuilder`
//! call sites cast to `u16` where `nostr::Kind::from` requires it.

/// NIP-60: Cashu wallet event — encrypted wallet config (privkey + mints).
pub use nmp_kinds::KIND_NIP60_WALLET as KIND_WALLET;

/// NIP-60: Cashu wallet unspent proof — encrypted token event.
pub use nmp_kinds::KIND_NIP60_TOKEN as KIND_TOKEN;

/// NIP-60: Cashu spending history event.
pub use nmp_kinds::KIND_NIP60_HISTORY as KIND_HISTORY;

/// NIP-60: Cashu wallet redeeming a quote (deposit in-progress).
pub use nmp_kinds::KIND_NIP60_QUOTE as KIND_QUOTE;

/// NIP-61: Cashu nutzap informational event — advertises accepted mints + pubkey.
pub use nmp_kinds::KIND_NIP61_NUTZAP_INFO as KIND_NUTZAP_INFO;

/// NIP-61: Cashu nutzap event — sends ecash proofs to a recipient.
pub use nmp_kinds::KIND_NIP61_NUTZAP as KIND_NUTZAP;

/// NIP-88: Mint announcement — mint publishes its metadata to Nostr.
pub use nmp_kinds::KIND_MINT_ANNOUNCE;
