//! Blueprint half (write side) per `docs/design/kind-wrappers.md` §3.2.
//!
//! Per-list-type builders that consume `self` (Rust idiom — no setter
//! mutation, no D4 violation) and produce an `UnsignedEvent`. The builders are
//! **pure**: no clock, no signer, no relay picking, no encryption. The action
//! ledger signs + publishes downstream.
//!
//! ## Why per-type entry points over one mega-builder
//!
//! The task brief asks for "the protocol modules a real social app needs":
//! `MuteList::builder()`, `BookmarkList::builder()`, `RelayList::builder()`,
//! and the set builders `FollowSet::new(d)` / `RelaySet::new(d)` /
//! `BookmarkSet::new(d)`. They share a private generic [`ListBuilder`] core
//! that emits tags in canonical order, but each entry point fixes its own kind
//! so callers cannot construct a "kind-less list" — the §9 #3 anti-pattern in
//! builder form.
//!
//! ## Private entries
//!
//! These builders emit only PUBLIC tags and leave `content` empty. NIP-51
//! private entries are NIP-04-encrypted JSON in `.content`; encryption needs
//! the user's key, which is an *actor* concern, not a pure builder concern
//! (mirrors the decoder's `encrypted_payload` verbatim-preserve contract).

use nmp_core::substrate::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::decode::RelayEntry;
use crate::kinds::{
    is_parameterized, KIND_BOOKMARK_LIST, KIND_BOOKMARK_SETS, KIND_FOLLOW_SETS, KIND_MUTE_LIST,
    KIND_RELAY_LIST, KIND_RELAY_SETS,
};

/// Structured builder errors per **D6** (errors never cross FFI as panics;
/// they become typed values).
///
/// `MissingDTag` fires only for the three parameterized set kinds (30000 /
/// 30002 / 30003) when their `d` identifier is empty or whitespace-only — an
/// empty `d` would silently collapse every set of that kind by the author into
/// one replaceable row. The three `1000x` list kinds never produce this error
/// (they are replaceable and intentionally carry no `d`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Nip51BuildError {
    /// A set kind (30000 / 30002 / 30003) was built with an empty or
    /// whitespace-only `d` identifier.
    MissingDTag,
}

impl core::fmt::Display for Nip51BuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingDTag => write!(
                f,
                "NIP-51 set (kind 30000/30002/30003) requires a non-empty `d` identifier"
            ),
        }
    }
}

impl std::error::Error for Nip51BuildError {}

/// Private generic core. Not public — callers reach it only through the
/// per-type entry points so a kind is always fixed.
#[derive(Clone, Debug)]
pub struct ListBuilder {
    kind: u32,
    d_tag: Option<String>,
    title: Option<String>,
    description: Option<String>,
    image: Option<String>,
    pubkeys: Vec<String>,
    events: Vec<String>,
    addresses: Vec<String>,
    hashtags: Vec<String>,
    relays: Vec<RelayEntry>,
    words: Vec<String>,
}

impl ListBuilder {
    fn new(kind: u32, d_tag: Option<String>) -> Self {
        Self {
            kind,
            d_tag,
            title: None,
            description: None,
            image: None,
            pubkeys: Vec::new(),
            events: Vec::new(),
            addresses: Vec::new(),
            hashtags: Vec::new(),
            relays: Vec::new(),
            words: Vec::new(),
        }
    }

    /// Set the `title` tag (intended for set kinds; harmless on list kinds).
    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }

    /// Set the `description` tag.
    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    /// Set the `image` tag.
    #[must_use]
    pub fn image(mut self, value: impl Into<String>) -> Self {
        self.image = Some(value.into());
        self
    }

    /// Add a public `p` (pubkey) entry.
    #[must_use]
    pub fn pubkey(mut self, pubkey: impl Into<String>) -> Self {
        self.pubkeys.push(pubkey.into());
        self
    }

    /// Add a public `e` (event id) entry.
    #[must_use]
    pub fn event(mut self, event_id: impl Into<String>) -> Self {
        self.events.push(event_id.into());
        self
    }

    /// Add a public `a` (address coordinate, `kind:pubkey:d`) entry.
    #[must_use]
    pub fn address(mut self, coord: impl Into<String>) -> Self {
        self.addresses.push(coord.into());
        self
    }

    /// Add a public `t` (hashtag) entry.
    #[must_use]
    pub fn hashtag(mut self, tag: impl Into<String>) -> Self {
        self.hashtags.push(tag.into());
        self
    }

    /// Add a public relay entry (`r` tag with optional read/write marker).
    #[must_use]
    pub fn relay(mut self, url: impl Into<String>, marker: Option<String>) -> Self {
        self.relays.push(RelayEntry {
            url: url.into(),
            marker,
        });
        self
    }

    /// Add a public `word` (muted word) entry.
    #[must_use]
    pub fn word(mut self, word: impl Into<String>) -> Self {
        self.words.push(word.into());
        self
    }

    /// Materialise the `UnsignedEvent`. For set kinds, validates the `d`
    /// identifier is non-empty after trim. `content` is left empty — private
    /// entries are encrypted by the actor downstream, never here.
    pub fn build(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, Nip51BuildError> {
        let mut tags: Vec<Vec<String>> = Vec::new();

        if is_parameterized(self.kind) {
            let d = self.d_tag.unwrap_or_default();
            if d.trim().is_empty() {
                return Err(Nip51BuildError::MissingDTag);
            }
            tags.push(vec!["d".into(), d]);
        }

        if let Some(title) = self.title {
            tags.push(vec!["title".into(), title]);
        }
        if let Some(description) = self.description {
            tags.push(vec!["description".into(), description]);
        }
        if let Some(image) = self.image {
            tags.push(vec!["image".into(), image]);
        }
        for pk in self.pubkeys {
            tags.push(vec!["p".into(), pk]);
        }
        for ev in self.events {
            tags.push(vec!["e".into(), ev]);
        }
        for addr in self.addresses {
            tags.push(vec!["a".into(), addr]);
        }
        for t in self.hashtags {
            tags.push(vec!["t".into(), t]);
        }
        for relay in self.relays {
            let mut tag = vec!["r".to_string(), relay.url];
            if let Some(marker) = relay.marker {
                tag.push(marker);
            }
            tags.push(tag);
        }
        for w in self.words {
            tags.push(vec!["word".into(), w]);
        }

        Ok(UnsignedEvent {
            pubkey: author.into(),
            kind: self.kind,
            tags,
            content: String::new(),
            created_at,
        })
    }
}

/// Entry point for a kind:10000 mute list (replaceable, no `d`).
pub struct MuteList;
impl MuteList {
    /// Fresh mute-list builder.
    #[must_use]
    pub fn builder() -> ListBuilder {
        ListBuilder::new(KIND_MUTE_LIST, None)
    }
}

/// Entry point for a kind:10003 bookmark list (replaceable, no `d`).
pub struct BookmarkList;
impl BookmarkList {
    /// Fresh bookmark-list builder.
    #[must_use]
    pub fn builder() -> ListBuilder {
        ListBuilder::new(KIND_BOOKMARK_LIST, None)
    }
}

/// Entry point for a kind:10002 relay list (replaceable, no `d`).
pub struct RelayList;
impl RelayList {
    /// Fresh relay-list builder.
    #[must_use]
    pub fn builder() -> ListBuilder {
        ListBuilder::new(KIND_RELAY_LIST, None)
    }
}

/// Entry point for a kind:30000 follow set (parameterized — `d` required).
pub struct FollowSet;
impl FollowSet {
    /// Fresh follow-set builder keyed on `d_tag`.
    ///
    /// Returns `ListBuilder` (not `Self`): `FollowSet` is a zero-field
    /// namespace for the entry constructor, mirroring nip23's `Article::new`.
    /// The brief asks for `FollowSet::new(d)` ergonomics, so the clippy
    /// `new_ret_no_self` lint is allowed here for the same reason.
    #[allow(clippy::new_ret_no_self)]
    #[must_use]
    pub fn new(d_tag: impl Into<String>) -> ListBuilder {
        ListBuilder::new(KIND_FOLLOW_SETS, Some(d_tag.into()))
    }
}

/// Entry point for a kind:30002 relay set (parameterized — `d` required).
pub struct RelaySet;
impl RelaySet {
    /// Fresh relay-set builder keyed on `d_tag`. See [`FollowSet::new`] for the
    /// `new_ret_no_self` rationale.
    #[allow(clippy::new_ret_no_self)]
    #[must_use]
    pub fn new(d_tag: impl Into<String>) -> ListBuilder {
        ListBuilder::new(KIND_RELAY_SETS, Some(d_tag.into()))
    }
}

/// Entry point for a kind:30003 bookmark set (parameterized — `d` required).
pub struct BookmarkSet;
impl BookmarkSet {
    /// Fresh bookmark-set builder keyed on `d_tag`. See [`FollowSet::new`] for
    /// the `new_ret_no_self` rationale.
    #[allow(clippy::new_ret_no_self)]
    #[must_use]
    pub fn new(d_tag: impl Into<String>) -> ListBuilder {
        ListBuilder::new(KIND_BOOKMARK_SETS, Some(d_tag.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHOR: &str = "deadbeef";

    #[test]
    fn mute_list_emits_kind_and_no_d_tag() {
        let ev = MuteList::builder()
            .pubkey("spammer")
            .word("crypto")
            .build(AUTHOR, 100)
            .unwrap();
        assert_eq!(ev.kind, KIND_MUTE_LIST);
        assert_eq!(ev.content, "");
        let keys: Vec<&str> = ev
            .tags
            .iter()
            .filter_map(|t| t.first())
            .map(String::as_str)
            .collect();
        assert!(!keys.contains(&"d"), "replaceable list has no d tag");
        assert_eq!(keys, vec!["p", "word"]);
    }

    #[test]
    fn bookmark_list_emits_kind_10003() {
        let ev = BookmarkList::builder()
            .event("ev1")
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(ev.kind, KIND_BOOKMARK_LIST);
        assert_eq!(ev.tags, vec![vec!["e".to_string(), "ev1".to_string()]]);
    }

    #[test]
    fn relay_list_emits_kind_10002_with_markers() {
        let ev = RelayList::builder()
            .relay("wss://a", Some("write".into()))
            .relay("wss://b", None)
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(ev.kind, KIND_RELAY_LIST);
        assert_eq!(
            ev.tags,
            vec![
                vec!["r".to_string(), "wss://a".to_string(), "write".to_string()],
                vec!["r".to_string(), "wss://b".to_string()],
            ]
        );
    }

    #[test]
    fn follow_set_emits_kind_30000_d_first_then_metadata_then_items() {
        let ev = FollowSet::new("friends")
            .title("Friends")
            .description("close")
            .image("https://i")
            .pubkey("pk1")
            .build(AUTHOR, 7)
            .unwrap();
        assert_eq!(ev.kind, KIND_FOLLOW_SETS);
        let keys: Vec<&str> = ev
            .tags
            .iter()
            .filter_map(|t| t.first())
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["d", "title", "description", "image", "p"]);
        assert_eq!(ev.tags[0], vec!["d".to_string(), "friends".to_string()]);
    }

    #[test]
    fn relay_set_and_bookmark_set_carry_their_kinds() {
        let rs = RelaySet::new("s").build(AUTHOR, 0).unwrap();
        assert_eq!(rs.kind, KIND_RELAY_SETS);
        let bs = BookmarkSet::new("s").build(AUTHOR, 0).unwrap();
        assert_eq!(bs.kind, KIND_BOOKMARK_SETS);
    }

    #[test]
    fn set_with_empty_d_returns_missing_d_tag() {
        assert_eq!(
            FollowSet::new("").build(AUTHOR, 0).unwrap_err(),
            Nip51BuildError::MissingDTag
        );
    }

    #[test]
    fn set_with_whitespace_d_returns_missing_d_tag() {
        assert_eq!(
            RelaySet::new("  \t").build(AUTHOR, 0).unwrap_err(),
            Nip51BuildError::MissingDTag
        );
        assert_eq!(
            BookmarkSet::new("\n").build(AUTHOR, 0).unwrap_err(),
            Nip51BuildError::MissingDTag
        );
    }

    #[test]
    fn replaceable_list_with_no_entries_is_valid() {
        // No d, no items — still a valid (empty) replaceable list.
        MuteList::builder()
            .build(AUTHOR, 0)
            .expect("empty mute list is valid");
    }

    #[test]
    fn builder_consumes_self_immutable_chain() {
        // Compile-time anti-NDK guarantee: every method takes `self` by value,
        // so no caller can retain a mutable handle to mutate tags after build.
        let _: UnsignedEvent = FollowSet::new("x").pubkey("p").build(AUTHOR, 0).unwrap();
    }

    #[test]
    fn error_display_is_human_readable() {
        let msg = format!("{}", Nip51BuildError::MissingDTag);
        assert!(msg.contains("`d`"));
        assert!(msg.contains("NIP-51"));
    }

    #[test]
    fn canonical_item_order_is_p_e_a_t_r_word() {
        let ev = MuteList::builder()
            .word("w")
            .relay("wss://r", None)
            .hashtag("t")
            .address("30000:pk:d")
            .event("ev")
            .pubkey("pk")
            .build(AUTHOR, 0)
            .unwrap();
        let keys: Vec<&str> = ev
            .tags
            .iter()
            .filter_map(|t| t.first())
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["p", "e", "a", "t", "r", "word"]);
    }
}
