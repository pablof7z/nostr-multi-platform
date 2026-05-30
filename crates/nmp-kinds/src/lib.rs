//! Canonical Nostr kind-integer registry for the NMP workspace.
//!
//! # Why a separate crate
//!
//! `nmp-core` (Layer 3) holds the kernel substrate; `nmp-nip59` (Layer 4)
//! holds the gift-wrap primitive. The kernel depends on `nmp-nip59`
//! (`nmp-core/Cargo.toml`), so `nmp-nip59` CANNOT import from `nmp-core`
//! without creating a compile-time cycle.  Both crates need `KIND_GIFT_WRAP`
//! = 1059.  The same cycle blocks every other NIP-4 crate that wants the
//! constants from `nmp-core::kinds`.
//!
//! Moving the integer registry one layer down — to this zero-dependency
//! Layer-0 crate, using `nmp-nip42-types` as structural precedent — resolves
//! the cycle: `nmp-kinds` has NO workspace dependencies, so nothing can cycle
//! through it.  `nmp-core::kinds` re-exports everything with `pub use
//! nmp_kinds::*` so ALL existing `nmp_core::kinds::KIND_*` call sites compile
//! unchanged.  `nmp-nip59`, `nmp-marmot`, and any other NIP crate simply add
//! `nmp-kinds` to their `[dependencies]` and import from here.
//!
//! # Precedent
//!
//! `nmp-nip42-types` is the direct structural precedent: a tiny zero-dep
//! Layer-0 vocabulary crate that exists specifically to break the cycle
//! between the kernel FSM and the NIP-42 protocol module.  `nmp-kinds` is
//! identical in purpose — vocabulary that two layers need, with no deps of
//! its own.
//!
//! # Future: `nmp-proto`
//!
//! `docs/architecture/crate-boundaries.md` plans a `nmp-proto` crate (Layer 0)
//! that re-exports all of upstream `nostr`.  If `nmp-proto` lands, it can
//! re-export or absorb `nmp-kinds`; that migration is orthogonal to this one.
//!
//! # Scope
//!
//! This crate is the workspace's canonical *integer* registry only.  Per-NIP
//! event-shape, parser, builder, and routing logic still lives in the
//! protocol crates; nothing about a constant being declared here implies
//! the kernel knows how to read or write the corresponding event.

// ─── NIP-01 — basic event kinds ────────────────────────────────────────────

/// NIP-01 profile metadata (kind:0).
pub const KIND_PROFILE_METADATA: u32 = 0;

/// NIP-01 short text note (kind:1).
pub const KIND_SHORT_TEXT_NOTE: u32 = 1;

/// NIP-02 contact list / follow set (kind:3).
pub const KIND_CONTACT_LIST: u32 = 3;

/// NIP-25 reaction (kind:7).
pub const KIND_REACTION: u32 = 7;

/// NIP-17 chat message rumor (kind:14). The unencrypted inner payload of a
/// gift-wrap envelope.
pub const KIND_CHAT_MESSAGE: u32 = 14;

// ─── NIP-59 — sealed gift-wrap chain ──────────────────────────────────────

/// NIP-59 gift-wrap envelope (kind:1059). The outer event minted by the
/// gift-wrap builder; the kernel's `publish_signed_event` D10 guard refuses
/// to Auto-route this kind to the author's NIP-65 outbox (the unlinkability
/// the construction exists to provide depends on the explicit relay pin).
pub const KIND_GIFT_WRAP: u32 = 1059;

// ─── NIP-65 — relay list metadata ─────────────────────────────────────────

/// NIP-65 relay list (kind:10002). The replaceable event each user
/// publishes to advertise their preferred read/write relays — the source
/// of truth the outbox resolver reads when routing a publish through
/// `PublishTarget::Auto`.
pub const KIND_RELAY_LIST: u32 = 10002;
