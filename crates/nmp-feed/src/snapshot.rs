//! [`RootCard`] and [`RootFeedSnapshot`] — the generic feed projection surface.
//!
//! A `RootCard<C>` wraps a single render card `C`. There is NO per-row
//! attribution list: the "who replied" rollup (`RootCard.attribution` /
//! the reply-attribution payload) was DELETED with the baked note/reply engine (#3082),
//! not re-homed. An app that wants a reply digest rebuilds it from the delivered
//! rows.
//!
//! These types are kind-agnostic wire/projection shapes carrying no protocol
//! convention. They were relocated here from the former `root_indexed` module.

use crate::{FeedPage, FeedWindowMetrics};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// One feed row: a single render card. (Formerly `RootCard<C, A>` with an
/// attribution list — the attribution list is gone.)
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(bound(serialize = "C: Serialize", deserialize = "C: DeserializeOwned"))]
pub struct RootCard<C>
where
    C: Clone + Serialize,
{
    /// The instance's render payload for the row event.
    pub card: C,
}

/// The visible-window projection the FFI surface serializes for a render
/// surface. Carries only the cards inside the current window (D5), the paging
/// envelope, and optional timing metrics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(bound(serialize = "C: Serialize", deserialize = "C: DeserializeOwned"))]
pub struct RootFeedSnapshot<C>
where
    C: Clone + Serialize,
{
    /// Visible-window cards, newest-first.
    pub cards: Vec<RootCard<C>>,
    /// Paging envelope (next cursor, has-more), `None` when the window is
    /// empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<FeedPage>,
    /// Optional snapshot-build timing metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<FeedWindowMetrics>,
}
