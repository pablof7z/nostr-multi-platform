//! NIP-23 kind constants.
//!
//! Per the NIP-23 spec (<https://github.com/nostr-protocol/nips/blob/master/23.md>),
//! long-form text content uses kind `30023` and is parameterized-replaceable
//! on the `d` tag (per NIP-33). Kind `30024` is the optional draft variant —
//! not handled by this crate (drafts live in `nmp-nip37` per the design
//! roadmap in `docs/design/kind-wrappers.md` §4).

/// NIP-23 long-form article kind.
pub const KIND_LONG_FORM_ARTICLE: u32 = 30023;
