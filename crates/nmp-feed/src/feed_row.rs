//! [`FeedRow`] — the generic, kind-agnostic feed row (successor to the demolished
//! note-only `NoteFeedItem`).
//!
//! # PROVISIONAL SHAPE — TODO(#3082)
//!
//! The exact final field set and the typed-context union are an OPEN design
//! decision tracked in #3082. This is a reasonable minimal shape chosen to
//! unblock the demolition; treat every field here as provisional until #3082
//! settles. In particular the FlatBuffers wire for this type is NOT yet
//! regenerated (see `nmp-feed/schema/` — none exists yet); the provisional
//! typed sidecar encodes via serde until the wire shape is frozen.
//!
//! # What this type is (and is not)
//!
//! `FeedRow` carries only RAW protocol facts — ids, pubkeys, kinds, timestamps,
//! raw content, raw tags, relay provenance — plus:
//!   * an optional [`RenderTarget`] pointer: "render a DIFFERENT event than the
//!     one that matched" (a repost renders its target). Resolving the pointer is
//!     NOT the feed's job — the feed DECLARES the target ref through the D7
//!     author-refs lane (see [`CardAuthors::rendered_target_refs`]) and the
//!     existing reactive `resolve_ref` primitive (ADR-0070) resolves it. The
//!     feed NEVER calls `resolve_ref` itself: demanding refs would tie target
//!     liveness to window churn (refcount coupling; #3082 point 5).
//!   * a list of typed [`FeedRowContext`] provenance entries (who reposted / who
//!     replied), carried as DATA, not as a note-only bolt-on.
//!
//! It carries NO display strings, NO parsed content tree, NO profile display,
//! NO counts, and NO previews — those are owned by the component/concept read
//! that asks for them. NIP-29 hosted-group identity is carried opaquely as data
//! ([`FeedRowContext::Group`]); the canonical typed form is the NIP-29 group-id type (owned by its protocol crate)
//! and is NOT duplicated as a typed struct in this low crate.

use serde::{Deserialize, Serialize};

use crate::author_refs::CardAuthors;

/// A generic feed row. Used as the card type `C` of [`crate::FlatFeed`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FeedRow {
    /// Canonical row identity — the dedup key. Defaults to `event.id`; a
    /// repost-aware app keys this by the repost TARGET id (computed by the
    /// NIP-18 parser), so the wrapper and target collapse into one row.
    pub id: String,
    /// The contributing event id (the wrapper for a repost). Lets a merge
    /// remove one contribution and recompute the row from the rest.
    pub source_id: String,
    /// Raw author pubkey (hex). For a repost row this is the TARGET author; the
    /// reposter is carried in [`FeedRowContext::Repost`].
    pub author_pubkey: String,
    /// Raw event kind.
    pub kind: u32,
    /// Raw event `created_at`.
    pub created_at: u64,
    /// Raw event content (never a display string).
    pub content: String,
    /// Raw event tags. Components do NIP-10/threading at render time; the feed
    /// engine stays ignorant of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Vec<String>>,
    /// Genuine network relays this event was received from (never the
    /// local-publish self-echo).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relay_provenance: Vec<String>,
    /// Optional pointer to a DIFFERENT event/address to render instead of the
    /// matched event. Resolved lazily via `resolve_ref`, NOT by the feed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_target: Option<RenderTarget>,
    /// Typed provenance context (repost / hosted-group). Carried as DATA. Empty
    /// for a plain top-level event.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<FeedRowContext>,
}

/// A typed pointer to a render target (an event or a replaceable address).
///
/// PROVISIONAL — TODO(#3082): the address variant fields may change once the
/// pointer-source read model (#2113) alignment is finalized.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderTarget {
    /// A concrete event id target (e.g. a NIP-18 repost target).
    Event {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event_kind: Option<u32>,
    },
    /// A replaceable/addressable target (`kind:author:identifier`).
    Address {
        address_kind: u32,
        author: String,
        identifier: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
    },
}

/// Typed provenance context attached to a [`FeedRow`].
///
/// PROVISIONAL — TODO(#3082): the exact variant set of this union is OPEN. There
/// is intentionally NO `Reply` / reply-digest variant — reply rollup was deleted,
/// not re-homed. NIP-29 group context is carried as opaque data (`Group`), NOT
/// as a typed group-id struct: that canonical type is owned by the NIP-29
/// protocol crate and must not be duplicated into this low crate.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "context", rename_all = "snake_case")]
pub enum FeedRowContext {
    /// This row was surfaced by a NIP-18 repost wrapper. Populated purely from
    /// the wrapper's OWN tags/embedded content (wrapper-local; never a store
    /// peek at the target — that was the cache-luck bug #3083).
    Repost {
        /// The reposter's pubkey.
        author_pubkey: String,
        /// The reposted note's own `created_at`.
        note_created_at: u64,
    },
    /// NIP-29 hosted-group context, carried opaquely as data. The canonical
    /// typed form is the NIP-29 group-id type (owned by its protocol crate).
    Group {
        /// The host relay URL (NIP-29 `h`-tag host).
        relay: String,
        /// The raw group id.
        id: String,
    },
}

impl CardAuthors for FeedRow {
    fn rendered_author_keys(&self) -> Vec<String> {
        let mut keys = vec![self.author_pubkey.clone()];
        for ctx in &self.context {
            if let FeedRowContext::Repost { author_pubkey, .. } = ctx {
                keys.push(author_pubkey.clone());
            }
        }
        keys
    }

    /// The render-target ref this row DECLARES for auto-resolution (a repost's
    /// target event id). Declared through the D7 lane; the feed never demands it.
    fn rendered_target_refs(&self) -> Vec<String> {
        match &self.render_target {
            Some(RenderTarget::Event { id, .. }) => vec![id.clone()],
            Some(RenderTarget::Address { .. }) | None => Vec::new(),
        }
    }
}

impl FeedRow {
    /// Build a plain top-level row from an event (identity = `event.id`).
    ///
    /// This is the DEFAULT identity/sort. Repost-aware or reply-rollup apps wrap
    /// this and override `id` / `render_target` / `context` on the four knobs.
    #[must_use]
    pub fn from_event(event: &nmp_core::substrate::KernelEvent) -> Self {
        Self {
            id: event.id.clone(),
            source_id: event.id.clone(),
            author_pubkey: event.author.clone(),
            kind: event.kind,
            created_at: event.created_at,
            content: event.content.clone(),
            tags: event.tags.clone(),
            relay_provenance: event.received_from_relays(),
            render_target: None,
            context: Vec::new(),
        }
    }
}
