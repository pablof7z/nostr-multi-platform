//! Canonical Nostr kind constants for the workspace.
//!
//! Centralized in `nmp-core` so the kernel and every protocol/app crate
//! consume a single source of truth for the integer kind numbers used on
//! the wire. Protocol crates that previously defined private duplicates
//! (`nmp-nip59::kinds::KIND_GIFT_WRAP`, `nmp-nip17::dm_relay_list::KIND_DM_RELAY_LIST`,
//! `nmp-marmot::interest::KIND_GIFT_WRAP`, `nmp-router::publish_relay_list::KIND_RELAY_LIST`,
//! …) should import from here instead — see V-57 P2 in `docs/BACKLOG.md` for
//! the migration plan and the per-crate follow-ups.
//!
//! # Why centralised
//!
//! V-57 P2 originally framed kind constants in `nmp-core` as a D0 violation
//! ("the kernel must not grow protocol nouns"). The project owner reframed
//! the rule on 2026-05-27: an integer is wire-protocol data, not an app or
//! protocol *noun*. Concentrating the integers in one file removes the
//! duplication risk (the kernel and `nmp-nip59` defining the same number in
//! two places, drifting on edit) without growing the kernel's *semantic*
//! surface — the kernel still does not decode tags, build per-kind events,
//! or run kind-specific business logic.
//!
//! # Scope
//!
//! This file is the workspace's canonical *integer* registry only. Per-NIP
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
