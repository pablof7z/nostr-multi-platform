//! Raw NIP-22 comment (kind:1111) decode.
//!
//! A NIP-22 comment carries two scopes of reference tags:
//!
//! - **Root scope** — UPPERCASE `A` / `E` / `I` identifying the artifact the
//!   whole thread hangs off, with a companion uppercase `K` carrying the root
//!   kind. The root is constant for every comment in a thread.
//! - **Parent scope** — lowercase `a` / `e` / `i` identifying the immediate
//!   parent (the comment being replied to), with a companion lowercase `k`
//!   carrying the parent kind. A top-level comment's parent *is* the root, so
//!   its parent scope mirrors the root.
//!
//! This module owns parsing only. It produces a flat [`CommentRecord`] of raw
//! protocol facts — no display strings, no labels, no counts (D1: projections
//! and decoders hold raw data; presentation belongs in the shell).

use nmp_core::substrate::KernelEvent;
use nmp_kinds::KIND_NIP22_COMMENT;
use serde::{Deserialize, Serialize};

/// A single decoded NIP-22 comment. All fields are raw protocol values exactly
/// as they appear on the wire (after trimming empties); no presentation.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommentRecord {
    /// kind:1111 event id (hex).
    pub event_id: String,
    /// Comment author pubkey (hex).
    pub author_pubkey: String,
    /// Raw comment body (event `content`), untouched.
    pub body: String,
    /// Root scope tag name — uppercase `A` / `E` / `I` (empty if absent).
    pub root_tag_name: String,
    /// Root scope tag value — address / event-id / external-id.
    pub root_tag_value: String,
    /// Root kind from the uppercase `K` tag (empty if absent).
    pub root_kind: String,
    /// Parent scope tag name — lowercase `a` / `e` / `i`. Mirrors the root tag
    /// (lowercased) for top-level comments.
    pub parent_tag_name: String,
    /// Parent scope tag value. Mirrors the root value for top-level comments.
    pub parent_tag_value: String,
    /// Parent kind from the lowercase `k` tag (empty if absent).
    pub parent_kind: String,
    /// Event `created_at` (unix seconds).
    pub created_at: u64,
}

impl CommentRecord {
    /// Whether this comment is top-level — its parent scope value equals the
    /// root scope value (NIP-22: a top-level comment's parent is the root).
    #[must_use]
    pub fn is_top_level(&self) -> bool {
        !self.root_tag_value.is_empty() && self.parent_tag_value == self.root_tag_value
    }
}

/// Decode a kernel event into a [`CommentRecord`], or `None` if it is not a
/// well-formed kind:1111 comment (wrong kind, or missing a root scope tag).
#[must_use]
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<CommentRecord> {
    if event.kind != KIND_NIP22_COMMENT {
        return None;
    }

    // Root scope — first uppercase A/E/I wins. NIP-22 permits multiples for
    // redundancy, but a single root is the common shape.
    let (root_tag_name, root_tag_value) = first_scope_tag(&event.tags, &["A", "E", "I"])?;

    // Parent scope — first lowercase a/e/i. Absent on top-level comments where
    // the parent is the root itself; fall back to the (lowercased) root so the
    // tree builder can always thread.
    let (parent_tag_name, parent_tag_value) = first_scope_tag(&event.tags, &["a", "e", "i"])
        .unwrap_or_else(|| (root_tag_name.to_ascii_lowercase(), root_tag_value.clone()));

    let root_kind = first_tag_value(&event.tags, "K").unwrap_or_default();
    let parent_kind = first_tag_value(&event.tags, "k").unwrap_or_else(|| root_kind.clone());

    Some(CommentRecord {
        event_id: event.id.clone(),
        author_pubkey: event.author.clone(),
        body: event.content.clone(),
        root_tag_name,
        root_tag_value,
        root_kind,
        parent_tag_name,
        parent_tag_value,
        parent_kind,
        created_at: event.created_at,
    })
}

/// First tag whose name is one of `names`, returning `(name, value)` with a
/// non-empty value.
fn first_scope_tag(tags: &[Vec<String>], names: &[&str]) -> Option<(String, String)> {
    tags.iter().find_map(|tag| {
        let name = tag.first()?;
        if !names.iter().any(|candidate| candidate == name) {
            return None;
        }
        let value = tag.get(1).filter(|value| !value.is_empty())?;
        Some((name.clone(), value.clone()))
    })
}

/// First value of the tag named `name`, if non-empty.
fn first_tag_value(tags: &[Vec<String>], name: &str) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|candidate| candidate == name) {
            tag.get(1).filter(|value| !value.is_empty()).cloned()
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
