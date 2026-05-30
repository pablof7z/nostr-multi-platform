//! Canonical Nostr kind constants for the workspace.
//!
//! The actual `pub const` definitions live in the zero-dependency Layer-0
//! crate `nmp-kinds`; this module re-exports them so ALL existing
//! `nmp_core::kinds::KIND_*` call sites across the workspace continue to
//! compile unchanged.
//!
//! # Why `nmp-kinds` (Layer 0), not here (Layer 3)
//!
//! `nmp-core` depends on `nmp-nip59` (the kernel uses the gift-wrap
//! primitive on the actor thread — ADR-0009 precedent). That edge means
//! `nmp-nip59 → nmp-core` would be a **compile-time cycle**, so `nmp-nip59`
//! cannot import `KIND_GIFT_WRAP` from this module directly. Moving the
//! integer registry to `nmp-kinds` (zero deps, Layer 0 — same pattern as
//! `nmp-nip42-types`) lets both `nmp-core` and `nmp-nip59` depend on the
//! same source without any cycle.
//!
//! # Scope
//!
//! This module is the workspace's canonical *integer* registry only. Per-NIP
//! event-shape, parser, builder, and routing logic still lives in the
//! protocol crates; nothing about a constant being declared here implies
//! the kernel knows how to read or write the corresponding event.

pub use nmp_kinds::*;
