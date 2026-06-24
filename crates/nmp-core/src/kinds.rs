//! Canonical Nostr kind constants for the workspace.
//!
//! The actual `pub const` definitions live in the zero-dependency Layer-0
//! crate `nmp-kinds`; this module re-exports them so ALL existing
//! `nmp_core::kinds::KIND_*` call sites across the workspace continue to
//! compile unchanged.
//!
//! # Why `nmp-kinds` (Layer 0), not here (Layer 3)
//!
//! `KIND_GIFT_WRAP` is shared by both `nmp-core` (acceptance tests) and the
//! Layer-0 gift-wrap crate `nmp-nip59`. If the constant lived here (Layer 3),
//! `nmp-nip59` could not import it without a `nmp-nip59 → nmp-core` edge that
//! would invert the layer order. Keeping the integer registry in `nmp-kinds`
//! (zero deps, Layer 0 — same pattern as `nmp-nip42-types`) lets both crates
//! depend on the same source without any cycle or upward edge.
//!
//! # Scope
//!
//! This module is the workspace's canonical *integer* registry only. Per-NIP
//! event-shape, parser, builder, and routing logic still lives in the
//! protocol crates; nothing about a constant being declared here implies
//! the kernel knows how to read or write the corresponding event.

// The integer constants AND the NIP-01 `is_replaceable` /
// `is_addressable` predicates both live in `nmp-kinds` (the
// zero-dep Layer-0 crate) so there is exactly ONE definition of "replaceable"
// in the workspace. A prior local copy here returned `true` for kind:1/6/7
// (treating 0–9999 as replaceable) — the OPPOSITE of `nostr::Kind` and of the
// `nmp-store` / `nmp-nostr-lmdb` predicates, a latent correctness hazard
// (#1493). The single canonical definition removes the divergence; all
// existing `nmp_core::kinds::is_replaceable` call sites resolve here unchanged.
pub use nmp_kinds::*;
