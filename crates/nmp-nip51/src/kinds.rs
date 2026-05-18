//! NIP-51 kind constants.
//!
//! Per the NIP-51 spec (<https://github.com/nostr-protocol/nips/blob/master/51.md>),
//! lists come in two storage shapes:
//!
//! - **Replaceable** standard lists (`10000`–`19999`): exactly one per kind per
//!   author, no `d` tag.
//! - **Parameterized-replaceable** sets (`30000`–`39999`): keyed by a `d` tag
//!   so an author can hold many of each kind (per NIP-33).
//!
//! This crate handles exactly the six kinds the task brief enumerates. Kind
//! `10002` (relay list) overlaps NIP-65 deliberately per the brief — the
//! kernel-resident NIP-65 path remains the outbox source of truth; this crate's
//! decoder is a pure read-side projection for list-aware UIs and never feeds
//! routing.

/// Mute list (kind 10000) — replaceable. Public muted entries are tags; private
/// ones are NIP-04-encrypted in `.content` (not decoded here — see
/// [`crate::decode`]).
pub const KIND_MUTE_LIST: u32 = 10000;

/// Relay list (kind 10002) — replaceable. Overlaps NIP-65; see the module docs.
pub const KIND_RELAY_LIST: u32 = 10002;

/// Bookmark list (kind 10003) — replaceable.
pub const KIND_BOOKMARK_LIST: u32 = 10003;

/// Follow sets (kind 30000) — parameterized-replaceable (`d` tag required).
pub const KIND_FOLLOW_SETS: u32 = 30000;

/// Relay sets (kind 30002) — parameterized-replaceable (`d` tag required).
pub const KIND_RELAY_SETS: u32 = 30002;

/// Bookmark sets (kind 30003) — parameterized-replaceable (`d` tag required).
pub const KIND_BOOKMARK_SETS: u32 = 30003;

/// Every kind this crate decodes / routes / registers, in ascending order.
pub const ALL_KINDS: &[u32] = &[
    KIND_MUTE_LIST,
    KIND_RELAY_LIST,
    KIND_BOOKMARK_LIST,
    KIND_FOLLOW_SETS,
    KIND_RELAY_SETS,
    KIND_BOOKMARK_SETS,
];

/// `true` when `kind` is one of the three parameterized-replaceable set kinds
/// (which require a non-empty `d` tag). The three `1000x` kinds are plain
/// replaceable and carry `d_tag == ""`.
#[must_use]
pub fn is_parameterized(kind: u32) -> bool {
    matches!(
        kind,
        KIND_FOLLOW_SETS | KIND_RELAY_SETS | KIND_BOOKMARK_SETS
    )
}
