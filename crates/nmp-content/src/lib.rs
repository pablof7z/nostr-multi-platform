//! `nmp-content` — Layer A content-rendering substrate.
//!
//! Pure-Rust tokenizer + entity resolver + embed-fetch deduplicator for Nostr
//! event content. See [`docs/design/content-rendering.md`] (§5 Layer A spec)
//! for the architectural rationale.
//!
//! # Layered API
//! - [`tokenize`] — pure function: `(content, tags, mode) -> ContentTree`
//! - [`segment::Segment`] / [`segment::ContentTree`] — the rendered IR every
//!   platform consumes
//! - [`mode::RenderMode`] — `Plain | Markdown | Auto` (Auto sniffs by kind)
//! - [`context::RenderContext`] — depth + visited-set recursion guard
//! - [`embed_registry::EmbedClaimRegistry`] — per-id refcounted claim/release
//!   (ViewModule per ADR-0009, namespace `nmp.content.embed_registry`)
//!
//! # Design constraints (load-bearing)
//! - **One entry point** (`tokenize`) with a `mode` flag — never the three
//!   overlapping APIs NDKSwift ships (`ndkswift.md` §10 anti-pattern #1).
//! - **One parser shape** — Markdown blocks recursively contain the same
//!   inline `Segment` variants; the plaintext and markdown render paths share
//!   tokenization (`content-rendering.md` §10 #3).
//! - **FFI-stable public types** — pulse-builder agent (#66) consumes these
//!   shapes via FFI; do not break.
//! - **D0-clean** — no UI nouns; `EmbedClaimRegistry` stays behind the
//!   `ViewModule` boundary.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod context;
pub mod embed_registry;
pub mod markdown;
pub mod mode;
pub mod segment;
pub mod wire;
mod grouper;
mod regex_set;
mod tokenizer;

pub use context::{render_context_can_descend, RenderContext};
pub use embed_registry::{
    ClaimHandle, EmbedClaimDelta, EmbedClaimRegistry, EmbedClaimSpec, EmbedClaimState,
    EmbedRegistrySnapshot, EmbedTarget,
};
pub use markdown::{MarkdownInline, MarkdownNode};
pub use mode::{sniff_mode_from_kind, RenderMode};
pub use segment::{ContentTree, InvoiceKind, MediaKind, Segment};
pub use tokenizer::{tokenize, tokenize_with_kind};
pub use wire::{
    ContentTreeWire, PlaceholderReason, WireNode, WireNostrUri, WireNostrUriKind,
    WIRE_MAX_DEPTH,
};
