//! `ModulePolicy` — knobs for the grouping algorithm. Spec carries one per
//! view instance so the same crate can serve Chirp (tight modules) and
//! podcast (longer ancestor chains) without an algorithmic fork.

use serde::{Deserialize, Serialize};

/// Tunables for [`crate::Grouper`]. Defaults mirror Twitter / X behaviour:
/// at most three messages per module, ancestor walk capped at two hops,
/// adjacent same-root modules merged.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModulePolicy {
    /// Maximum events surfaced inside a single `TimelineBlock::Module`.
    /// Excess is rendered as additional standalone or chained modules.
    pub max_module_size: u8,
    /// Time gap (seconds) between adjacent module events before the block
    /// is marked `has_gap = true`. Defaults to 72h.
    pub max_lookback_gap_secs: u64,
    /// How many ancestor hops to walk when stitching a reply into its
    /// parent chain. `Address` / `External` pointers terminate the walk
    /// regardless of the remaining budget.
    pub max_ancestor_hops: u8,
    /// Whether adjacent modules that share the same root pointer should be
    /// merged into one block (Twitter-style "this is the same thread").
    pub collapse_adjacent_same_root: bool,
}

impl Default for ModulePolicy {
    fn default() -> Self {
        Self {
            max_module_size: 3,
            max_lookback_gap_secs: 72 * 3600,
            max_ancestor_hops: 2,
            collapse_adjacent_same_root: true,
        }
    }
}
