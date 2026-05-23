//! `TimelineBlock` — the grouper's output unit. A timeline payload is a
//! `Vec<TimelineBlock>`; each block renders either as one standalone event
//! card or as a Twitter-style stacked module with a connecting vertical line.

use nmp_core::substrate::EventId;
use serde::{Deserialize, Serialize};

use crate::pointer::ThreadPointer;

/// Either one event on its own, or a chained module of contextually related
/// events (root-first newest-last when fully chained; see [`crate::Grouper`]).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TimelineBlock {
    Standalone(EventId),
    Module {
        /// Event ids in display order: root-first (oldest) to leaf (newest).
        events: Vec<EventId>,
        /// True when an ancestor in the chain is missing from the local store
        /// OR the lookback between adjacent events exceeded `ModulePolicy
        /// ::max_lookback_gap_secs` OR the chain's resolved root pointer is
        /// not the top event's id.
        has_gap: bool,
        /// Terminal root pointer used for adjacent-module collapse. `None`
        /// when the module's top event is itself a thread root.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root: Option<ThreadPointer>,
    },
}

impl TimelineBlock {
    /// Length of the block in events (1 for standalone).
    #[must_use] 
    pub fn len(&self) -> usize {
        match self {
            Self::Standalone(_) => 1,
            Self::Module { events, .. } => events.len(),
        }
    }

    /// True when the block carries no events. Always `false` in practice —
    /// the grouper never emits empty modules.
    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
