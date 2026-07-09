//! Composite-feed declaration surface (#3082 settled design).
//!
//! A composite feed is an ADDITIVE SET OF LANES over one `FlatFeed`-style
//! engine — the same engine `FeedParams` compiles to (`crates/nmp-feed/src/flat.rs`).
//! Today's single-scope [`crate::FeedParams`] feed is the degenerate one-lane
//! case; nothing about it changes. Each lane names an acquisition scope, a
//! kind/tag match filter, and an opaque, registered mapping id — never a
//! native closure (D0). The closures a [`LaneMappingId`] names are constructed
//! in Rust AT THE COMPOSITION ROOT (ADR-0069) and never cross FFI, the same
//! discipline as [`crate::CustomAdmissionId`].

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::params::{FeedItemProjection, FeedScope, FeedWindowPolicy, ProjectionKey};

/// A raw nostr tag name (`"e"`, `"a"`, `"K"`, `"k"`, ...). Newtype so a lane's
/// tag-match map cannot be confused with an arbitrary string map.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct TagKey(pub String);

/// Opaque, app/protocol-registered lane-mapping id.
///
/// Resolving this id (at the composition root, never over FFI) yields a pure
/// `KernelEvent -> Vec<MappedRow>` closure with the SAME determinism contract
/// as [`crate::FlatFeedItemBuilder`]: a function of the delivered event only,
/// no store peek. Protocol crates register extraction mappings under
/// framework-owned ids of their own choosing (e.g. a repost-target extractor,
/// a comment-root extractor); `nmp-feed` registers only the kind-blind
/// identity mapping (`feed.authored`, [`DIRECT_MAPPING_ID`]). The engine never
/// learns a kind — mapping selection is entirely the app/protocol composition
/// root's responsibility.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct LaneMappingId(pub String);

impl LaneMappingId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The framework-owned identity mapping id `nmp-feed` registers: kind-blind,
/// `canonical_row_id = event.id`, `FeedRowContext::Authored` provenance. This
/// is the zero-config default lane mapping ([`FeedLane::direct`]) — "render
/// the source" holds by construction.
pub const DIRECT_MAPPING_ID: &str = "feed.authored";

/// One additive lane in a [`CompositeFeedParams`] declaration.
///
/// A lane names WHERE events come from (`source`), WHICH events this lane
/// claims (`match_kinds` / `match_tags`), and HOW a claimed event becomes a
/// row (`mapping`). Multiple lanes may claim the same event (e.g. a kind that
/// is both a primary content kind and a pointer-wrapper kind in another feed);
/// the compiler runs every matching lane's mapping and the engine's arity-Vec
/// item builder (`FlatFeedItemBuilder`) ingests all of them.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FeedLane {
    pub source: FeedScope,
    pub match_kinds: Vec<u32>,
    #[serde(default)]
    pub match_tags: BTreeMap<TagKey, BTreeSet<String>>,
    pub mapping: LaneMappingId,
}

impl FeedLane {
    /// The zero-config default lane: renders the source directly via
    /// `nmp-feed`'s own [`DIRECT_MAPPING_ID`] mapping.
    #[must_use]
    pub fn direct(source: FeedScope, match_kinds: Vec<u32>) -> Self {
        Self {
            source,
            match_kinds,
            match_tags: BTreeMap::new(),
            mapping: LaneMappingId(DIRECT_MAPPING_ID.to_string()),
        }
    }
}

/// How a composite feed orders its admitted, deduped rows.
///
/// `sort_key` MUST be a pure function of delivered sources in both cases
/// (never re-read from a store), so resumable `(created_at, id)` cursor
/// paging stays correct.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum SortPolicy {
    /// Newest interaction wins the row's sort position — a later repost/comment
    /// bumps the row to the top, same as the pre-composite follows-timeline
    /// repost "bump" behavior.
    ByInteractionTime,
    /// The row sorts by its DELIVERED target's own `created_at` once known
    /// (e.g. an article's real publish time); before delivery, a provisional
    /// interaction-time proxy holds the row's position.
    ByTargetCreatedAt,
}

/// The full typed declaration an app submits to open a composite feed.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CompositeFeedParams {
    /// App-owned output projection key.
    pub key: ProjectionKey,
    /// The additive lane set.
    pub lanes: Vec<FeedLane>,
    /// The kinds a `Delivered` ref's target is allowed to admit as (e.g.
    /// `[30023]` for an article-composite). This is DISTINCT from any lane's
    /// `match_kinds`: a lane's match kinds name the ACQUIRED wrapper/pointer
    /// kind, while `render_target_kinds` names the DELIVERED target's own
    /// kind — the engine folds a `Delivered` ref's target into this SAME
    /// session's admission only when its kind is in this set.
    pub render_target_kinds: Vec<u32>,
    pub sort: SortPolicy,
    pub window: FeedWindowPolicy,
    pub item_projection: FeedItemProjection,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_lane_uses_the_framework_identity_mapping() {
        let lane = FeedLane::direct(FeedScope::ActiveUserFollows, vec![1]);
        assert_eq!(lane.mapping, LaneMappingId(DIRECT_MAPPING_ID.to_string()));
        assert!(lane.match_tags.is_empty());
    }
}
