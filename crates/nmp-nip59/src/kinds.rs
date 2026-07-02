//! NIP-59 gift-wrap kinds — re-exported from the canonical Layer-0 registry.
//!
//! The actual `pub const KIND_GIFT_WRAP` definition lives in `nmp-kinds`
//! (a zero-dependency Layer-0 crate). This module re-exports it so that
//! `nmp_nip59::KIND_GIFT_WRAP` (and `nmp_nip59::kinds::KIND_GIFT_WRAP`) keep
//! resolving for all existing downstream importers without change.
//!
//! Why not define it here? `nmp-nip59` is itself a **Layer-0** substrate-grade
//! crate (`docs/architecture/crate-boundaries.md` §2/§8: pure seal/wrap/unwrap
//! over the `nostr` crate, only a `nmp-kinds` workspace dep) and must not
//! depend on `nmp-core` (Layer 3) — that direction would be a layer inversion
//! and, historically, a compile cycle (`nmp-core` needs `KIND_GIFT_WRAP` too,
//! and defining the constant in `nmp-core::kinds` for `nmp-nip59` to import
//! back would create `nmp-nip59 -> nmp-core -> nmp-nip59`). Moving the
//! constant to `nmp-kinds` (Layer 0, zero deps) lets both `nmp-nip59` and any
//! kernel/protocol consumer read the same integer from one shared Layer-0
//! source instead. `nmp-core` today has no production dependency on
//! `nmp-nip59` at all — the seal+wrap path lives in `nmp-nip17`; `nmp-core`
//! consumes `nmp-nip59` only as a dev-dependency for its acceptance harness.
//! See V-57 P2.

pub use nmp_kinds::KIND_GIFT_WRAP;
