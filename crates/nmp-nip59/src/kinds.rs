//! NIP-59 gift-wrap kinds — re-exported from the canonical Layer-0 registry.
//!
//! The actual `pub const KIND_GIFT_WRAP` definition lives in `nmp-kinds`
//! (a zero-dependency Layer-0 crate). This module re-exports it so that
//! `nmp_nip59::KIND_GIFT_WRAP` (and `nmp_nip59::kinds::KIND_GIFT_WRAP`) keep
//! resolving for all existing downstream importers without change.
//!
//! Why not define it here? `nmp-core` (Layer 3) depends on `nmp-nip59`
//! (Layer 4); `nmp-core` also needs `KIND_GIFT_WRAP`. Defining the constant
//! in `nmp-core::kinds` and having `nmp-nip59` import it would create a
//! compile cycle. Moving it to `nmp-kinds` (Layer 0, zero deps) lets both
//! layers consume the same integer from one source. See V-57 P2.

pub use nmp_kinds::KIND_GIFT_WRAP;
