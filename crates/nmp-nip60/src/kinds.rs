//! Kind constants for NIP-60 (Cashu wallet) and related NIPs.
//!
//! The kind *integers* are the workspace canon in `nmp-kinds` (the zero-dep
//! Layer-0 registry). Re-export the canonical names directly so all callers
//! use the single source of truth without per-crate aliases.

pub use nmp_kinds::{
    KIND_MINT_ANNOUNCE,
    KIND_NIP60_HISTORY,
    KIND_NIP60_QUOTE,
    KIND_NIP60_TOKEN,
    KIND_NIP60_WALLET,
    KIND_NIP61_NUTZAP,
    KIND_NIP61_NUTZAP_INFO,
};
