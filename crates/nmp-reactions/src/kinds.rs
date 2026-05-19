//! NIP-25 / NIP-18 kind constants.
//!
//! - NIP-25 reactions (<https://github.com/nostr-protocol/nips/blob/master/25.md>)
//!   use kind `7`. `content` is `"+"` (like), `"-"` (dislike), an emoji, or a
//!   `:shortcode:` resolved by an accompanying `emoji` tag.
//! - NIP-18 reposts (<https://github.com/nostr-protocol/nips/blob/master/18.md>)
//!   use kind `6` to repost a kind:1 note (`.content` carries the stringified
//!   original event JSON, or is empty) and kind `16` ("generic repost") to
//!   repost any other kind, carrying a `k` tag whose value is the stringified
//!   original kind.
//!
//! Per the NIP wire spec, kinds 7 / 6 / 16 are **regular events** — they are
//! NOT replaceable. There is no parameterized-replaceable `d`-tag supersession
//! for these; the analogue of nip23's "stale redelivery" guard is plain
//! duplicate-`event_id` idempotency (see [`crate::domain`]).

/// NIP-25 reaction kind.
pub const KIND_REACTION: u32 = 7;

/// NIP-18 repost kind — reposts a kind:1 short text note.
pub const KIND_REPOST: u32 = 6;

/// NIP-18 generic-repost kind — reposts any kind; carries a `k` tag holding
/// the stringified original kind.
pub const KIND_GENERIC_REPOST: u32 = 16;

/// The complete set of kinds this crate decodes / ingests. Order is the
/// canonical `[7, 6, 16]` the task brief and the `DomainModule::ingest_kinds`
/// contract use.
pub const SOCIAL_KINDS: &[u32] = &[KIND_REACTION, KIND_REPOST, KIND_GENERIC_REPOST];
