//! Blueprint half — `Comment::on_event(root_id, root_kind, root_author)`
//! returns a [`CommentBuilder`] for a top-level comment; nest with
//! `.reply_to_comment(parent_id, parent_kind, parent_author)`. Builders
//! consume `self` on every chain link (Rust D4 idiom) and produce an
//! `UnsignedEvent` — no signer, no clock.

use nmp_core::substrate::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_COMMENT;

/// Structured builder errors per **D6**.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CommentBuildError {
    /// `content` is empty / whitespace-only.
    EmptyContent,
    /// Missing root (uppercase `E`/`A`/`I`). NIP-22 requires a root scope.
    MissingRoot,
}

impl core::fmt::Display for CommentBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyContent => write!(f, "NIP-22 comment requires non-empty content"),
            Self::MissingRoot => write!(f, "NIP-22 comment requires a root scope (E/A/I)"),
        }
    }
}

impl std::error::Error for CommentBuildError {}

/// Anchor for the root or parent slot of a NIP-22 comment. The two slots use
/// different tag casing (uppercase vs lowercase) but share this shape.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Anchor {
    Event {
        id: String,
        kind: u32,
        author: Option<String>,
        relay: Option<String>,
    },
    Address {
        coord: String,
        kind: u32,
        author: Option<String>,
        relay: Option<String>,
    },
    External {
        uri: String,
    },
}

impl Anchor {
    fn emit(&self, uppercase: bool, tags: &mut Vec<Vec<String>>) {
        let (e_key, a_key, i_key, k_key, p_key) = if uppercase {
            ("E", "A", "I", "K", "P")
        } else {
            ("e", "a", "i", "k", "p")
        };
        match self {
            Self::Event { id, kind, author, relay } => {
                let mut t = vec![e_key.to_string(), id.clone()];
                if let Some(r) = relay {
                    t.push(r.clone());
                }
                tags.push(t);
                tags.push(vec![k_key.to_string(), kind.to_string()]);
                if let Some(a) = author {
                    let mut p = vec![p_key.to_string(), a.clone()];
                    if let Some(r) = relay {
                        p.push(r.clone());
                    }
                    tags.push(p);
                }
            }
            Self::Address { coord, kind, author, relay } => {
                let mut t = vec![a_key.to_string(), coord.clone()];
                if let Some(r) = relay {
                    t.push(r.clone());
                }
                tags.push(t);
                tags.push(vec![k_key.to_string(), kind.to_string()]);
                if let Some(a) = author {
                    let mut p = vec![p_key.to_string(), a.clone()];
                    if let Some(r) = relay {
                        p.push(r.clone());
                    }
                    tags.push(p);
                }
            }
            Self::External { uri } => {
                tags.push(vec![i_key.to_string(), uri.clone()]);
            }
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Event { id, .. } => id.trim().is_empty(),
            Self::Address { coord, .. } => coord.trim().is_empty(),
            Self::External { uri } => uri.trim().is_empty(),
        }
    }
}

/// Entry-point namespace.
pub struct Comment;

impl Comment {
    /// Start a top-level comment on a regular Nostr event (e.g. an article
    /// kind:30023, a note kind:1, or any other kind). Root and parent point
    /// at the same target.
    pub fn on_event(
        root_id: impl Into<String>,
        root_kind: u32,
        root_author: impl Into<String>,
    ) -> CommentBuilder {
        let root_id = root_id.into();
        let root_author = root_author.into();
        let root = Anchor::Event {
            id: root_id,
            kind: root_kind,
            author: Some(root_author),
            relay: None,
        };
        CommentBuilder {
            content: String::new(),
            root: root.clone(),
            parent: root,
            relay_hint: None,
        }
    }

    /// Start a top-level comment on an addressable (NIP-33) target.
    pub fn on_address(
        coord: impl Into<String>,
        root_kind: u32,
        root_author: impl Into<String>,
    ) -> CommentBuilder {
        let root = Anchor::Address {
            coord: coord.into(),
            kind: root_kind,
            author: Some(root_author.into()),
            relay: None,
        };
        CommentBuilder {
            content: String::new(),
            root: root.clone(),
            parent: root,
            relay_hint: None,
        }
    }

    /// Start a top-level comment on an external resource (e.g. a URL).
    pub fn on_external(uri: impl Into<String>) -> CommentBuilder {
        let root = Anchor::External { uri: uri.into() };
        CommentBuilder {
            content: String::new(),
            root: root.clone(),
            parent: root,
            relay_hint: None,
        }
    }
}

/// Builder for a kind:1111 standalone comment.
#[derive(Clone, Debug)]
pub struct CommentBuilder {
    content: String,
    root: Anchor,
    parent: Anchor,
    relay_hint: Option<String>,
}

impl CommentBuilder {
    /// Set the comment body. Required (typed-error if empty at `.build`).
    #[must_use]
    pub fn content(mut self, v: impl Into<String>) -> Self {
        self.content = v.into();
        self
    }

    /// Single relay-hint applied to the `E`/`A`/`P`/`e`/`a`/`p` tags that
    /// carry one.
    #[must_use]
    pub fn relay_hint(mut self, relay: impl Into<String>) -> Self {
        let v = relay.into();
        let hint = if v.trim().is_empty() { None } else { Some(v) };
        // Apply to whichever anchor slots are mutable.
        if let Anchor::Event { ref mut relay, .. } = self.root {
            *relay = hint.clone();
        }
        if let Anchor::Address { ref mut relay, .. } = self.root {
            *relay = hint.clone();
        }
        if let Anchor::Event { ref mut relay, .. } = self.parent {
            *relay = hint.clone();
        }
        if let Anchor::Address { ref mut relay, .. } = self.parent {
            *relay = hint.clone();
        }
        self.relay_hint = hint;
        self
    }

    /// Nest under a parent comment. Root stays, parent switches to the
    /// comment being replied to (`parent_kind` is typically 1111).
    #[must_use]
    pub fn reply_to_comment(
        mut self,
        parent_id: impl Into<String>,
        parent_kind: u32,
        parent_author: impl Into<String>,
    ) -> Self {
        self.parent = Anchor::Event {
            id: parent_id.into(),
            kind: parent_kind,
            author: Some(parent_author.into()),
            relay: self.relay_hint.clone(),
        };
        self
    }

    /// Materialise the `UnsignedEvent`. Validates content + root.
    pub fn build(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, CommentBuildError> {
        if self.content.trim().is_empty() {
            return Err(CommentBuildError::EmptyContent);
        }
        if self.root.is_empty() {
            return Err(CommentBuildError::MissingRoot);
        }

        let mut tags: Vec<Vec<String>> = Vec::with_capacity(6);
        // Uppercase root scope, then lowercase parent scope. NIP-22 doesn't
        // mandate ordering, but a stable order keeps wire output and tests
        // deterministic.
        self.root.emit(/* uppercase= */ true, &mut tags);
        self.parent.emit(/* uppercase= */ false, &mut tags);

        Ok(UnsignedEvent {
            pubkey: author.into(),
            kind: KIND_COMMENT,
            tags,
            content: self.content,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHOR: &str = "deadbeef";

    fn tag_keys(unsigned: &UnsignedEvent) -> Vec<&str> {
        unsigned.tags.iter().filter_map(|t| t.first()).map(String::as_str).collect()
    }

    #[test]
    fn top_level_event_comment_emits_uppercase_and_lowercase_pointing_at_root() {
        let unsigned = Comment::on_event("ARTICLE", 30023, "alice")
            .content("first!")
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(unsigned.kind, KIND_COMMENT);
        let keys = tag_keys(&unsigned);
        assert_eq!(keys, vec!["E", "K", "P", "e", "k", "p"]);
        assert_eq!(unsigned.tags[0][1], "ARTICLE");
        assert_eq!(unsigned.tags[1][1], "30023");
        assert_eq!(unsigned.tags[2][1], "alice");
        assert_eq!(unsigned.tags[3][1], "ARTICLE");
        assert_eq!(unsigned.tags[4][1], "30023");
        assert_eq!(unsigned.tags[5][1], "alice");
    }

    #[test]
    fn nested_comment_swaps_parent_to_parent_comment() {
        let unsigned = Comment::on_event("ARTICLE", 30023, "alice")
            .reply_to_comment("PARENT_C", 1111, "bob")
            .content("re:")
            .build(AUTHOR, 0)
            .unwrap();
        let keys = tag_keys(&unsigned);
        assert_eq!(keys, vec!["E", "K", "P", "e", "k", "p"]);
        // Uppercase still points at root.
        assert_eq!(unsigned.tags[0][1], "ARTICLE");
        assert_eq!(unsigned.tags[2][1], "alice");
        // Lowercase points at parent comment.
        assert_eq!(unsigned.tags[3][1], "PARENT_C");
        assert_eq!(unsigned.tags[4][1], "1111");
        assert_eq!(unsigned.tags[5][1], "bob");
    }

    #[test]
    fn address_root_emits_a_tags() {
        let unsigned = Comment::on_address("30023:alice:intro", 30023, "alice")
            .content("x")
            .build(AUTHOR, 0)
            .unwrap();
        let keys = tag_keys(&unsigned);
        assert_eq!(keys, vec!["A", "K", "P", "a", "k", "p"]);
        assert_eq!(unsigned.tags[0][1], "30023:alice:intro");
    }

    #[test]
    fn external_root_emits_i_tags_with_no_author() {
        let unsigned = Comment::on_external("https://example.com/post")
            .content("good")
            .build(AUTHOR, 0)
            .unwrap();
        let keys = tag_keys(&unsigned);
        // No K/P for external roots, only I — and parent inherits.
        assert_eq!(keys, vec!["I", "i"]);
        assert_eq!(unsigned.tags[0][1], "https://example.com/post");
    }

    #[test]
    fn empty_content_errors() {
        let err = Comment::on_event("X", 1, "y").build(AUTHOR, 0).unwrap_err();
        assert_eq!(err, CommentBuildError::EmptyContent);
    }

    #[test]
    fn whitespace_root_errors() {
        let err = Comment::on_event("   ", 1, "y")
            .content("x")
            .build(AUTHOR, 0)
            .unwrap_err();
        assert_eq!(err, CommentBuildError::MissingRoot);
    }

    #[test]
    fn relay_hint_lands_on_event_pointers() {
        let unsigned = Comment::on_event("ARTICLE", 30023, "alice")
            .relay_hint("wss://r.x")
            .content("x")
            .build(AUTHOR, 0)
            .unwrap();
        // E tag, P tag, e tag, p tag all carry the relay column.
        assert_eq!(unsigned.tags[0][2], "wss://r.x");
        assert_eq!(unsigned.tags[2][2], "wss://r.x");
        assert_eq!(unsigned.tags[3][2], "wss://r.x");
        assert_eq!(unsigned.tags[5][2], "wss://r.x");
    }

    #[test]
    fn builder_consumes_self_compile_check() {
        let _: UnsignedEvent = Comment::on_event("X", 1, "y").content("c").build(AUTHOR, 0).unwrap();
    }
}
