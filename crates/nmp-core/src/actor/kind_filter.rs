//! Per-registration kind filter for event observers and external event sinks.

use std::collections::BTreeSet;

/// Per-registration kind filter. Empty → match every kind.
#[derive(Clone, Debug, Default)]
pub struct KindFilter(BTreeSet<u32>);

impl KindFilter {
    /// Build a filter from a kind list. An empty list yields the
    /// match-everything filter.
    #[must_use]
    pub fn from_kinds<I: IntoIterator<Item = u32>>(kinds: I) -> Self {
        Self(kinds.into_iter().collect())
    }

    /// `true` if `kind` should be delivered: either the filter is empty
    /// (match all) or `kind` is explicitly listed.
    #[must_use]
    pub fn matches(&self, kind: u32) -> bool {
        self.0.is_empty() || self.0.contains(&kind)
    }

    /// `true` when no kinds are listed (match-everything).
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.0.is_empty()
    }

    /// The explicit kinds as a `Vec<u32>` (ascending). Empty for a match-all
    /// filter. Used by the external-event-sink store-resync replay, which scans
    /// the `created_at` index by explicit kind.
    #[must_use]
    pub fn kinds_vec(&self) -> Vec<u32> {
        self.0.iter().copied().collect()
    }
}
