//! Projection output types for the pointer-source read model: the sort policy
//! and the per-target projected item.

use crate::embed_registry::{EmbedTarget, ResolvedEvent};

/// Projection ordering for the resolved target items.
///
/// Re-sorting is **read-model only**: [`PointerSourceModel::set_sort`] reorders
/// the projected output and never withdraws or reopens a pointer or target
/// interest (issue #2113 invariant: "sort changes recompute projection order
/// without reopening unchanged pointer or target interests").
///
/// [`PointerSourceModel::set_sort`]: super::PointerSourceModel::set_sort
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PointerSortMode {
    /// Newest target content first (`created_at` of the pointed-to event).
    #[default]
    Time,
    /// Most-recently-pointed-at first (newest pointer `created_at`).
    TagTime,
    /// Most-pointed-at first (number of distinct pointer events).
    Count,
    /// Most author-diverse first (distinct pointer authors).
    UniqueAuthor,
}

impl PointerSortMode {
    /// Order `items` in place under this mode. Ties break on the target event id
    /// so the projection order is deterministic across recomputations.
    pub(super) fn order(self, items: &mut [PointerItem]) {
        match self {
            Self::Time => items.sort_by(|a, b| {
                b.event
                    .created_at
                    .cmp(&a.event.created_at)
                    .then_with(|| a.event.id.cmp(&b.event.id))
            }),
            Self::TagTime => items.sort_by(|a, b| {
                b.latest_pointer_at
                    .cmp(&a.latest_pointer_at)
                    .then_with(|| a.event.id.cmp(&b.event.id))
            }),
            Self::Count => items.sort_by(|a, b| {
                b.pointer_count
                    .cmp(&a.pointer_count)
                    .then_with(|| a.event.id.cmp(&b.event.id))
            }),
            Self::UniqueAuthor => items.sort_by(|a, b| {
                b.unique_authors
                    .cmp(&a.unique_authors)
                    .then_with(|| a.event.id.cmp(&b.event.id))
            }),
        }
    }
}

/// One projected, hydrated target plus its pointer aggregates.
#[derive(Clone, Debug, PartialEq)]
pub struct PointerItem {
    /// Identity of the pointed-to content.
    pub target: EmbedTarget,
    /// The hydrated target event.
    pub event: ResolvedEvent,
    /// Number of distinct pointer events referencing this target.
    pub pointer_count: usize,
    /// Number of distinct pointer authors referencing this target.
    pub unique_authors: usize,
    /// Newest `created_at` among the pointer events referencing this target.
    pub latest_pointer_at: u64,
}
