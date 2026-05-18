//! Blueprint half (write side) per `docs/design/kind-wrappers.md` §3.2.
//!
//! Fluent builder that consumes `self` (Rust idiom — no setter mutation,
//! no D4 violation) and produces an `UnsignedEvent`. The builder is **pure**:
//! no clock reads, no signer, no relay picking. The action ledger turns the
//! `UnsignedEvent` into a signed + published event downstream.
//!
//! The design doc's signature is `into_unsigned(author, created_at)`; the task
//! brief asks for `.build()`. Reconciled to `.build(author, created_at)` — the
//! verb the task asked for, with the parameters `UnsignedEvent` requires
//! (`pubkey` and `created_at` per `nmp_core::substrate::UnsignedEvent`).
//! Inventing a clock or signer inside the builder would be the §3.2 anti-pattern.

use nmp_core::substrate::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_LONG_FORM_ARTICLE;

/// Structured builder errors per **D6** (errors never cross FFI as panics; they
/// become typed values).
///
/// `MissingDTag` fires when `d_tag` is empty after trim — the NIP-33 spec
/// requires a non-empty identifier for parameterized-replaceable events. An
/// empty `d` tag would silently collapse every published article into a single
/// replaceable row in the store.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ArticleBuildError {
    /// `d` tag is empty or whitespace-only.
    MissingDTag,
}

impl core::fmt::Display for ArticleBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingDTag => write!(f, "NIP-23 article requires a non-empty `d` tag"),
        }
    }
}

impl std::error::Error for ArticleBuildError {}

/// Builder for a NIP-23 article. See the module-level docs for design
/// rationale; in particular, `.build` takes `author` + `created_at` so the
/// builder stays pure (no `SystemTime::now`, no signer reference).
#[derive(Clone, Debug)]
pub struct ArticleBuilder {
    d_tag: String,
    title: Option<String>,
    image: Option<String>,
    summary: Option<String>,
    published_at: Option<u64>,
    content: String,
}

/// Entry-point convenience type so callers write
/// `Article::new(d).title(t).content(c).build(author, ts)`.
///
/// `Article::new` intentionally returns `ArticleBuilder` (not `Self`); the
/// type exists purely as a namespace for the entry-point constructor. The
/// design's "no shared mutable read/write wrapper" rule (§1) means there is
/// nothing for `Article` itself to hold — it is by design a unit. Clippy's
/// `new_ret_no_self` lint is allowed here because the alternative — a method
/// called something other than `new` — would diverge from the task brief's
/// `Article::new(...)` ergonomic.
pub struct Article;

impl Article {
    /// Create a fresh `ArticleBuilder` keyed on `d_tag`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(d_tag: impl Into<String>) -> ArticleBuilder {
        ArticleBuilder {
            d_tag: d_tag.into(),
            title: None,
            image: None,
            summary: None,
            published_at: None,
            content: String::new(),
        }
    }
}

impl ArticleBuilder {
    /// Set the article title (`title` tag).
    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }

    /// Set the cover image URL (`image` tag).
    #[must_use]
    pub fn image(mut self, value: impl Into<String>) -> Self {
        self.image = Some(value.into());
        self
    }

    /// Set the summary (`summary` tag).
    #[must_use]
    pub fn summary(mut self, value: impl Into<String>) -> Self {
        self.summary = Some(value.into());
        self
    }

    /// Set the original-publication time (`published_at` tag, unix seconds).
    #[must_use]
    pub fn published_at(mut self, unix_seconds: u64) -> Self {
        self.published_at = Some(unix_seconds);
        self
    }

    /// Set the article body (markdown).
    #[must_use]
    pub fn content(mut self, value: impl Into<String>) -> Self {
        self.content = value.into();
        self
    }

    /// Materialise the `UnsignedEvent`. Validates that `d_tag` is non-empty
    /// (after trim). The action ledger then signs + publishes per the existing
    /// pipeline; `Builder → UnsignedEvent` is the only contract owed here.
    pub fn build(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, ArticleBuildError> {
        if self.d_tag.trim().is_empty() {
            return Err(ArticleBuildError::MissingDTag);
        }

        let mut tags: Vec<Vec<String>> = Vec::with_capacity(5);
        tags.push(vec!["d".into(), self.d_tag]);
        if let Some(title) = self.title {
            tags.push(vec!["title".into(), title]);
        }
        if let Some(image) = self.image {
            tags.push(vec!["image".into(), image]);
        }
        if let Some(summary) = self.summary {
            tags.push(vec!["summary".into(), summary]);
        }
        if let Some(published_at) = self.published_at {
            tags.push(vec!["published_at".into(), published_at.to_string()]);
        }

        Ok(UnsignedEvent {
            pubkey: author.into(),
            kind: KIND_LONG_FORM_ARTICLE,
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

    #[test]
    fn build_minimal_only_emits_d_tag() {
        let unsigned = Article::new("intro").build(AUTHOR, 1_700_000_000).unwrap();
        assert_eq!(unsigned.kind, KIND_LONG_FORM_ARTICLE);
        assert_eq!(unsigned.tags, vec![vec!["d".to_string(), "intro".to_string()]]);
        assert_eq!(unsigned.content, "");
        assert_eq!(unsigned.pubkey, AUTHOR);
        assert_eq!(unsigned.created_at, 1_700_000_000);
    }

    #[test]
    fn build_with_title_emits_title_tag_after_d() {
        let unsigned = Article::new("intro")
            .title("Hello, World")
            .build(AUTHOR, 0)
            .unwrap();
        assert_eq!(
            unsigned.tags,
            vec![
                vec!["d".to_string(), "intro".to_string()],
                vec!["title".to_string(), "Hello, World".to_string()],
            ]
        );
    }

    #[test]
    fn build_with_every_field_emits_all_tags_in_canonical_order() {
        let unsigned = Article::new("intro")
            .title("T")
            .image("https://example.com/i.png")
            .summary("S")
            .published_at(1_690_000_000)
            .content("# Heading\n\nbody")
            .build(AUTHOR, 1_700_000_000)
            .unwrap();
        let tag_keys: Vec<&str> = unsigned.tags.iter().filter_map(|t| t.first()).map(|s| s.as_str()).collect();
        assert_eq!(tag_keys, vec!["d", "title", "image", "summary", "published_at"]);
        assert_eq!(unsigned.content, "# Heading\n\nbody");
    }

    #[test]
    fn build_empty_d_tag_returns_missing_d_tag_error() {
        let err = Article::new("").build(AUTHOR, 0).unwrap_err();
        assert_eq!(err, ArticleBuildError::MissingDTag);
    }

    #[test]
    fn build_whitespace_d_tag_returns_missing_d_tag_error() {
        let err = Article::new("   ").build(AUTHOR, 0).unwrap_err();
        assert_eq!(err, ArticleBuildError::MissingDTag);
    }

    #[test]
    fn build_published_at_serializes_as_string() {
        let unsigned = Article::new("intro")
            .published_at(42)
            .build(AUTHOR, 0)
            .unwrap();
        let published_at_tag = unsigned.tags.iter().find(|t| t.first().map(String::as_str) == Some("published_at")).unwrap();
        assert_eq!(published_at_tag[1], "42");
    }

    #[test]
    fn builder_is_immutable_chain_consume_self() {
        // Compile-time check: builder methods take self by value (no &mut), so
        // we cannot accidentally retain a mutable handle. This test is the
        // anti-NDK guarantee made executable.
        let _: UnsignedEvent = Article::new("x").title("y").build(AUTHOR, 0).unwrap();
    }

    #[test]
    fn error_display_is_human_readable() {
        let msg = format!("{}", ArticleBuildError::MissingDTag);
        assert!(msg.contains("d"));
    }
}
