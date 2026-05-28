//! [`RootCard`] and [`RootFeedSnapshot`] — the engine's projection surface.
//!
//! A `RootCard<C, A>` pairs a root's render card `C` with the raw list of
//! attributions `A`. Per the user's Q1 decision (2026-05-27d) there is NO cap
//! field and NO `attribution_total`: the `Vec<A>` length IS the count, and
//! each render surface chooses how many to show. The only bound on the vector
//! is the engine's per-root D5 sub-map cap.
//!
//! Explicit `C: Serialize + Clone` / `A: Serialize + Clone` bounds satisfy
//! codex M4 — the derives would otherwise infer the wrong bound set across the
//! generic params.

use crate::{FeedPage, FeedWindowMetrics};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// One feed row: a root render card plus its raw attribution list.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(bound(
    serialize = "C: Serialize, A: Serialize",
    deserialize = "C: DeserializeOwned, A: DeserializeOwned"
))]
pub struct RootCard<C, A>
where
    C: Clone + Serialize,
    A: Clone + Serialize,
{
    /// The protocol instance's render payload for the root event.
    pub card: C,
    /// Raw attributions (follows who referenced this root). Length is the
    /// count; no separate total. Bounded only by the engine's per-root D5 cap.
    pub attribution: Vec<A>,
}

/// The visible-window projection the FFI surface serializes for a render
/// surface. Carries only the cards inside the current window (D5), the paging
/// envelope, and optional timing metrics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(bound(
    serialize = "C: Serialize, A: Serialize",
    deserialize = "C: DeserializeOwned, A: DeserializeOwned"
))]
pub struct RootFeedSnapshot<C, A>
where
    C: Clone + Serialize,
    A: Clone + Serialize,
{
    /// Visible-window root cards, newest-first.
    pub cards: Vec<RootCard<C, A>>,
    /// Paging envelope (next cursor, has-more), `None` when the window is
    /// empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<FeedPage>,
    /// Optional snapshot-build timing metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<FeedWindowMetrics>,
}
