//! [`FeedRow`] — the generic, kind-agnostic feed row (successor to the demolished
//! note-only `NoteFeedItem`) — FROZEN shape (#3082).
//!
//! # What this type is (and is not)
//!
//! `FeedRow` carries only RAW protocol facts — ids, pubkeys, kinds, timestamps,
//! raw content, raw tags, relay provenance — plus:
//!   * a delivery-tagged [`TypedRef`] vector ([`FeedRow::refs`]): each ref names
//!     an event id or address AND whether the engine should DECLARE it for lazy
//!     render-only resolution ([`DeliveryMode::RenderOnly`], the existing
//!     quote/repost-target-preview lane) or fold it into THIS feed session's own
//!     delivery so it re-enters as a real event ([`DeliveryMode::Delivered`]).
//!     Resolving a `RenderOnly` ref is NOT the feed's job — the feed DECLARES it
//!     through the D7 author-refs lane (see [`CardAuthors::rendered_target_refs`])
//!     and the existing reactive `resolve_ref` primitive (ADR-0070) resolves it.
//!     The feed NEVER calls `resolve_ref` itself: demanding refs would tie
//!     target liveness to window churn (refcount coupling; #3082 point 5).
//!   * a list of typed [`FeedRowContext`] provenance entries (authored / who
//!     reposted / who commented / hosted-group), carried as DATA and
//!     ACCUMULATED as a SET across every source contributing to this canonical
//!     row (a composite feed's merge policy unions them; see
//!     `crates/nmp-feed-session` composite compiler).
//!
//! It carries NO display strings, NO parsed content tree, NO profile display,
//! NO counts, and NO previews — those are owned by the component/concept read
//! that asks for them. NIP-29 hosted-group identity is carried opaquely as data
//! ([`FeedRowContext::Group`]); the canonical typed form is the NIP-29 group-id
//! type (owned by its protocol crate) and is NOT duplicated as a typed struct in
//! this low crate.

use serde::{Deserialize, Serialize};

use crate::author_refs::CardAuthors;
use crate::typed_ref::DeliveryMode;
pub use crate::typed_ref::TypedRef;

/// A generic feed row. Used as the card type `C` of [`crate::FlatFeed`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FeedRow {
    /// Canonical row identity — the dedup key. Opaque to the engine; computed
    /// by the app/protocol layer. Defaults to `event.id` ([`FeedRow::from_event`]);
    /// a repost-aware or address-keyed mapping keys this by the target's own
    /// id/coordinate (or a group-scoped `coord@group`) so wrapper and target
    /// collapse into one row.
    pub canonical_row_id: String,
    /// The contributing event id (the wrapper for a repost, the comment for a
    /// NIP-22 mapping). Lets a merge remove one contribution and recompute the
    /// row from the rest.
    pub source_id: String,
    /// Raw author pubkey (hex). For a placeholder row surfaced by a
    /// provenance-only lane (repost/comment before its target is delivered)
    /// this is empty; the real author arrives once the target is delivered.
    pub author_pubkey: String,
    /// Raw event kind. `0` marks a not-yet-hydrated placeholder row (see
    /// [`FeedRow::is_placeholder`]).
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
    /// Delivery-tagged typed refs this row declares. At most one `Delivered`
    /// ref (see [`merge_refs`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<TypedRef>,
    /// Typed provenance context (authored / repost / comment / hosted-group).
    /// Carried as DATA and accumulated as a SET across sources. Empty only for
    /// a row whose provenance has not been classified (legacy single-lane
    /// callers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<FeedRowContext>,
}

/// Typed provenance context attached to a [`FeedRow`].
///
/// Accumulated as a SET across every source contributing to a canonical row
/// (e.g. a composite feed's article row can carry `Authored` + `RepostedBy` +
/// `CommentedBy` all at once). There is intentionally NO `Reply` /
/// reply-digest variant — reply rollup was deleted, not re-homed. NIP-29 group
/// context is carried as opaque data (`Group`), NOT as a typed group-id struct:
/// that canonical type is owned by the NIP-29 protocol crate and must not be
/// duplicated into this low crate.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(tag = "context", rename_all = "snake_case")]
pub enum FeedRowContext {
    /// This row was surfaced directly: the contributing event IS the row
    /// (a `feed.authored` / `Direct`-lane mapping).
    Authored,
    /// This row was surfaced by a NIP-18 repost wrapper. Populated purely from
    /// the wrapper's OWN tags/embedded content (wrapper-local; never a store
    /// peek at the target — that was the cache-luck bug #3083).
    RepostedBy {
        /// The reposter's pubkey.
        author_pubkey: String,
        /// The reposted note's own `created_at` (from embedded content, else
        /// the wrapper's own time as a placeholder proxy).
        note_created_at: u64,
    },
    /// This row was surfaced by a NIP-22 comment (kind:1111) whose root scope
    /// names this row's canonical target. Populated purely from the comment's
    /// own tags — never a store peek at the root.
    CommentedBy {
        /// The commenter's pubkey.
        author_pubkey: String,
        /// The kind:1111 comment event id.
        comment_event_id: String,
        /// The comment's own `created_at`.
        comment_created_at: u64,
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

/// Union two rows' provenance contexts, deduping exact-equal entries. Used by
/// composite merge policies to accumulate the provenance SET across sources.
#[must_use]
pub fn merge_context(
    existing: &[FeedRowContext],
    incoming: &[FeedRowContext],
) -> Vec<FeedRowContext> {
    let mut merged: Vec<FeedRowContext> = Vec::new();
    for ctx in existing.iter().chain(incoming.iter()) {
        if !merged.contains(ctx) {
            merged.push(ctx.clone());
        }
    }
    merged
}

impl CardAuthors for FeedRow {
    fn rendered_author_keys(&self) -> Vec<String> {
        let mut keys = vec![self.author_pubkey.clone()];
        for ctx in &self.context {
            match ctx {
                FeedRowContext::RepostedBy { author_pubkey, .. }
                | FeedRowContext::CommentedBy { author_pubkey, .. } => {
                    keys.push(author_pubkey.clone());
                }
                FeedRowContext::Authored | FeedRowContext::Group { .. } => {}
            }
        }
        keys
    }

    /// The `RenderOnly` refs this row DECLARES for auto-resolution (e.g. a
    /// repost/quote target event id). Declared through the D7 lane; the feed
    /// never demands them. `Delivered` refs are NOT declared here — they are
    /// absorbed by the feed session's own acquisition instead (never both).
    fn rendered_target_refs(&self) -> Vec<String> {
        self.refs
            .iter()
            .filter(|r| r.delivery_mode == DeliveryMode::RenderOnly)
            .filter_map(|r| r.target.event_id().map(str::to_string))
            .collect()
    }
}

impl FeedRow {
    /// Build a plain top-level row from an event (identity = `event.id`).
    ///
    /// This is the DEFAULT identity/sort. Repost-aware, comment-aware, or
    /// address-keyed mappings wrap this and override `canonical_row_id` /
    /// `refs` / `context` on the composable knobs.
    #[must_use]
    pub fn from_event(event: &nmp_core::substrate::KernelEvent) -> Self {
        Self {
            canonical_row_id: event.id.clone(),
            source_id: event.id.clone(),
            author_pubkey: event.author.clone(),
            kind: event.kind,
            created_at: event.created_at,
            content: event.content.clone(),
            tags: event.tags.clone(),
            relay_provenance: event.received_from_relays(),
            refs: Vec::new(),
            context: Vec::new(),
        }
    }

    /// Whether this row is an un-hydrated placeholder (a provenance-only lane
    /// surfaced it before its `Delivered` target arrived). `kind == 0` is the
    /// internal sentinel a placeholder-row constructor uses — `FeedRow` never
    /// carries a genuine kind:0 (profile metadata) row: this engine surfaces
    /// content items (notes, articles, pictures, comments), never raw profile
    /// events.
    #[must_use]
    pub fn is_placeholder(&self) -> bool {
        self.kind == 0
    }
}
