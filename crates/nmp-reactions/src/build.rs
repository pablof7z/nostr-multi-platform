//! Blueprint half (write side) per `docs/design/kind-wrappers.md` §3.2.
//!
//! Fluent builders that consume `self` (D4 — no setter mutation, no shared
//! mutable read/write wrapper) and produce an `UnsignedEvent`. The builders are
//! **pure**: no clock reads, no signer, no relay picking. The action ledger
//! turns the `UnsignedEvent` into a signed + published event downstream.
//!
//! The design doc's signature is `into_unsigned(author, created_at)`; the task
//! brief asks for `.to_event(...)` entry points. Reconciled to
//! `to_event(...)` / `to_address(...)` / `of(...)` constructors that take the
//! target up front, then `.build(author, created_at)` for the parameters
//! `UnsignedEvent` requires. Inventing a clock or signer inside the builder
//! would be the §3.2 anti-pattern.
//!
//! ## Canonical tag order
//!
//! Every builder emits tags in this fixed order so the wire form is
//! deterministic and round-trips cleanly through the decoder:
//!
//! `e` (target event id) · `p` (target author) · `a` (addressable coord, if
//! the target is an address) · `k` (original kind, generic repost only) ·
//! `emoji` (custom emoji, reaction only).

use nmp_core::planner::NaddrCoord;
use nmp_core::substrate::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::kinds::{KIND_GENERIC_REPOST, KIND_REACTION, KIND_REPOST};

/// Structured builder errors per **D6** (errors never cross FFI as panics;
/// they become typed values).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReactionBuildError {
    /// A reaction's `content` is empty — NIP-25 requires non-empty content
    /// (`"+"`, `"-"`, an emoji, or a `:shortcode:`).
    EmptyContent,
    /// The target event id (or addressable coordinate) is missing/empty — a
    /// reaction/repost with no target is meaningless and would index under an
    /// empty key.
    MissingTarget,
}

impl core::fmt::Display for ReactionBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyContent => {
                write!(
                    f,
                    "NIP-25 reaction requires non-empty content (e.g. `+`, `-`, an emoji)"
                )
            }
            Self::MissingTarget => {
                write!(
                    f,
                    "reaction/repost requires a non-empty target event id or address"
                )
            }
        }
    }
}

impl std::error::Error for ReactionBuildError {}

/// What a builder points at, mirroring [`crate::decode::ReactionTarget`] on the
/// write side.
#[derive(Clone, Debug, Eq, PartialEq)]
enum TargetSpec {
    Event(String),
    Address(NaddrCoord),
}

impl TargetSpec {
    fn is_empty(&self) -> bool {
        match self {
            TargetSpec::Event(id) => id.trim().is_empty(),
            TargetSpec::Address(c) => c.pubkey.trim().is_empty(),
        }
    }
}

/// Render the `a` tag value (`<kind>:<pubkey>:<d-tag>`) for an addressable
/// target.
fn addr_value(coord: &NaddrCoord) -> String {
    format!("{}:{}:{}", coord.kind, coord.pubkey, coord.d_tag)
}

// ─── Reaction (kind:7) ───────────────────────────────────────────────────────

/// Builder for a NIP-25 reaction (kind:7). Defaults `content` to `"+"` (the
/// canonical "like"); call [`ReactionBuilder::content`] to override and
/// [`ReactionBuilder::emoji`] to attach a NIP-30 custom emoji.
#[derive(Clone, Debug)]
pub struct ReactionBuilder {
    target: TargetSpec,
    target_author: Option<String>,
    content: String,
    emoji: Option<(String, String)>,
}

/// Entry-point namespace so callers write
/// `Reaction::to_event(id, author).content("❤️").build(pk, ts)`.
pub struct Reaction;

impl Reaction {
    /// React to a concrete event by hex id, with the target's author pubkey
    /// (NIP-25 SHOULD include the `p` tag).
    pub fn to_event(
        target_id: impl Into<String>,
        target_author: impl Into<String>,
    ) -> ReactionBuilder {
        ReactionBuilder {
            target: TargetSpec::Event(target_id.into()),
            target_author: Some(target_author.into()),
            content: "+".to_string(),
            emoji: None,
        }
    }

    /// React to an addressable event by coordinate.
    pub fn to_address(coord: NaddrCoord, target_author: impl Into<String>) -> ReactionBuilder {
        ReactionBuilder {
            target: TargetSpec::Address(coord),
            target_author: Some(target_author.into()),
            content: "+".to_string(),
            emoji: None,
        }
    }
}

impl ReactionBuilder {
    /// Override the reaction string (`"+"`, `"-"`, an emoji, or `:shortcode:`).
    #[must_use]
    pub fn content(mut self, value: impl Into<String>) -> Self {
        self.content = value.into();
        self
    }

    /// Attach a NIP-30 custom emoji (`["emoji", <shortcode>, <url>]`). The
    /// caller is expected to set `content` to `:shortcode:` to match.
    #[must_use]
    pub fn emoji(mut self, shortcode: impl Into<String>, url: impl Into<String>) -> Self {
        self.emoji = Some((shortcode.into(), url.into()));
        self
    }

    /// Materialise the `UnsignedEvent`. Validates non-empty `content` and a
    /// non-empty target per D6.
    pub fn build(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, ReactionBuildError> {
        if self.content.trim().is_empty() {
            return Err(ReactionBuildError::EmptyContent);
        }
        if self.target.is_empty() {
            return Err(ReactionBuildError::MissingTarget);
        }

        let mut tags: Vec<Vec<String>> = Vec::with_capacity(4);
        // NIP-25: a concrete target uses `e`; an addressable target uses `a`
        // (emitted after `p` per the canonical order). The two are mutually
        // exclusive — never both.
        if let TargetSpec::Event(id) = &self.target {
            tags.push(vec!["e".into(), id.clone()]);
        }
        if let Some(p) = &self.target_author {
            tags.push(vec!["p".into(), p.clone()]);
        }
        if let TargetSpec::Address(c) = &self.target {
            tags.push(vec!["a".into(), addr_value(c)]);
        }
        if let Some((shortcode, url)) = self.emoji {
            tags.push(vec!["emoji".into(), shortcode, url]);
        }

        Ok(UnsignedEvent {
            pubkey: author.into(),
            kind: KIND_REACTION,
            tags,
            content: self.content,
            created_at,
        })
    }
}

// ─── Repost (kind:6) ─────────────────────────────────────────────────────────

/// Builder for a NIP-18 repost (kind:6) of a kind:1 note. `.content` may carry
/// the stringified original event JSON; default is empty.
#[derive(Clone, Debug)]
pub struct RepostBuilder {
    target_id: String,
    target_author: Option<String>,
    embedded: String,
}

/// Entry-point namespace: `Repost::of(id, author).build(pk, ts)`.
pub struct Repost;

impl Repost {
    /// Repost a concrete kind:1 note by id + author.
    pub fn of(target_id: impl Into<String>, target_author: impl Into<String>) -> RepostBuilder {
        RepostBuilder {
            target_id: target_id.into(),
            target_author: Some(target_author.into()),
            embedded: String::new(),
        }
    }
}

impl RepostBuilder {
    /// Set the embedded original-event JSON (`.content`). Optional per NIP-18.
    #[must_use]
    pub fn embed(mut self, original_json: impl Into<String>) -> Self {
        self.embedded = original_json.into();
        self
    }

    /// Materialise the `UnsignedEvent`. Validates a non-empty target id.
    pub fn build(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, ReactionBuildError> {
        if self.target_id.trim().is_empty() {
            return Err(ReactionBuildError::MissingTarget);
        }
        let mut tags: Vec<Vec<String>> = Vec::with_capacity(2);
        tags.push(vec!["e".into(), self.target_id]);
        if let Some(p) = self.target_author {
            tags.push(vec!["p".into(), p]);
        }
        Ok(UnsignedEvent {
            pubkey: author.into(),
            kind: KIND_REPOST,
            tags,
            content: self.embedded,
            created_at,
        })
    }
}

// ─── GenericRepost (kind:16) ─────────────────────────────────────────────────

/// Builder for a NIP-18 generic repost (kind:16) of any kind. Emits a `k` tag
/// carrying the stringified original kind.
#[derive(Clone, Debug)]
pub struct GenericRepostBuilder {
    target_id: String,
    target_author: Option<String>,
    original_kind: u32,
    embedded: String,
}

/// Entry-point namespace:
/// `GenericRepost::of(id, author, 30023).build(pk, ts)`.
pub struct GenericRepost;

impl GenericRepost {
    /// Repost an event of `original_kind` by id + author.
    pub fn of(
        target_id: impl Into<String>,
        target_author: impl Into<String>,
        original_kind: u32,
    ) -> GenericRepostBuilder {
        GenericRepostBuilder {
            target_id: target_id.into(),
            target_author: Some(target_author.into()),
            original_kind,
            embedded: String::new(),
        }
    }
}

impl GenericRepostBuilder {
    /// Set the embedded original-event JSON (`.content`). Optional per NIP-18.
    #[must_use]
    pub fn embed(mut self, original_json: impl Into<String>) -> Self {
        self.embedded = original_json.into();
        self
    }

    /// Materialise the `UnsignedEvent`. Validates a non-empty target id. Emits
    /// tags in canonical order `e`, `p`, `k`.
    pub fn build(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, ReactionBuildError> {
        if self.target_id.trim().is_empty() {
            return Err(ReactionBuildError::MissingTarget);
        }
        let mut tags: Vec<Vec<String>> = Vec::with_capacity(3);
        tags.push(vec!["e".into(), self.target_id]);
        if let Some(p) = self.target_author {
            tags.push(vec!["p".into(), p]);
        }
        tags.push(vec!["k".into(), self.original_kind.to_string()]);
        Ok(UnsignedEvent {
            pubkey: author.into(),
            kind: KIND_GENERIC_REPOST,
            tags,
            content: self.embedded,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHOR: &str = "deadbeef";
    const TARGET: &str = "f00d";
    const TARGET_AUTHOR: &str = "cafe";

    fn tag_keys(ev: &UnsignedEvent) -> Vec<&str> {
        ev.tags
            .iter()
            .filter_map(|t| t.first())
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn reaction_defaults_to_plus_and_emits_e_p() {
        let ev = Reaction::to_event(TARGET, TARGET_AUTHOR)
            .build(AUTHOR, 1)
            .unwrap();
        assert_eq!(ev.kind, KIND_REACTION);
        assert_eq!(ev.content, "+");
        assert_eq!(tag_keys(&ev), vec!["e", "p"]);
        assert_eq!(ev.tags[0], vec!["e".to_string(), TARGET.to_string()]);
        assert_eq!(ev.tags[1], vec!["p".to_string(), TARGET_AUTHOR.to_string()]);
    }

    #[test]
    fn reaction_custom_content_and_emoji_canonical_order() {
        let ev = Reaction::to_event(TARGET, TARGET_AUTHOR)
            .content(":soapbox:")
            .emoji("soapbox", "https://x/soapbox.png")
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(ev.content, ":soapbox:");
        assert_eq!(tag_keys(&ev), vec!["e", "p", "emoji"]);
        assert_eq!(
            ev.tags[2],
            vec![
                "emoji".to_string(),
                "soapbox".to_string(),
                "https://x/soapbox.png".to_string()
            ]
        );
    }

    #[test]
    fn reaction_to_address_emits_a_after_p() {
        let coord = NaddrCoord {
            pubkey: "d".repeat(64),
            kind: 30023,
            d_tag: "art".into(),
        };
        let ev = Reaction::to_address(coord, TARGET_AUTHOR)
            .content("+")
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(tag_keys(&ev), vec!["p", "a"]);
        assert_eq!(ev.tags[1][1], format!("30023:{}:art", "d".repeat(64)));
    }

    #[test]
    fn reaction_empty_content_errors() {
        let err = Reaction::to_event(TARGET, TARGET_AUTHOR)
            .content("   ")
            .build(AUTHOR, 0)
            .unwrap_err();
        assert_eq!(err, ReactionBuildError::EmptyContent);
    }

    #[test]
    fn reaction_missing_target_errors() {
        let err = Reaction::to_event("", TARGET_AUTHOR)
            .build(AUTHOR, 0)
            .unwrap_err();
        assert_eq!(err, ReactionBuildError::MissingTarget);
    }

    #[test]
    fn repost_emits_kind_6_with_e_p() {
        let ev = Repost::of(TARGET, TARGET_AUTHOR).build(AUTHOR, 0).unwrap();
        assert_eq!(ev.kind, KIND_REPOST);
        assert_eq!(tag_keys(&ev), vec!["e", "p"]);
        assert_eq!(ev.content, "");
    }

    #[test]
    fn repost_preserves_embedded_json() {
        let json = r#"{"id":"x","kind":1}"#;
        let ev = Repost::of(TARGET, TARGET_AUTHOR)
            .embed(json)
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(ev.content, json);
    }

    #[test]
    fn repost_missing_target_errors() {
        let err = Repost::of("  ", TARGET_AUTHOR)
            .build(AUTHOR, 0)
            .unwrap_err();
        assert_eq!(err, ReactionBuildError::MissingTarget);
    }

    #[test]
    fn generic_repost_emits_kind_16_with_k_tag() {
        let ev = GenericRepost::of(TARGET, TARGET_AUTHOR, 30023)
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(ev.kind, KIND_GENERIC_REPOST);
        assert_eq!(tag_keys(&ev), vec!["e", "p", "k"]);
        assert_eq!(ev.tags[2], vec!["k".to_string(), "30023".to_string()]);
    }

    #[test]
    fn generic_repost_missing_target_errors() {
        let err = GenericRepost::of("", TARGET_AUTHOR, 1)
            .build(AUTHOR, 0)
            .unwrap_err();
        assert_eq!(err, ReactionBuildError::MissingTarget);
    }

    #[test]
    fn builder_is_immutable_chain_consume_self() {
        // Compile-time check: builder methods take self by value (no &mut), so
        // we cannot retain a mutable handle. The anti-NDK guarantee made
        // executable.
        let _: UnsignedEvent = Reaction::to_event(TARGET, TARGET_AUTHOR)
            .content("❤️")
            .build(AUTHOR, 0)
            .unwrap();
    }

    #[test]
    fn error_display_is_human_readable() {
        assert!(format!("{}", ReactionBuildError::EmptyContent).contains("content"));
        assert!(format!("{}", ReactionBuildError::MissingTarget).contains("target"));
    }
}
