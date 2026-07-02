use serde::{Deserialize, Serialize};

/// Mechanical reason a feed load/drain stopped.
///
/// These are feed-engine facts, not host policy. Hosts may render different UI
/// for them, but retry, budget, and source semantics stay Rust-owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedLoadStopReason {
    /// The requested page/window target was filled.
    WindowFilled,
    /// The source reported no more rows for this feed.
    SourceExhausted,
    /// The per-drain source scan cap was reached before the source exhausted.
    SourceScanBudgetReached,
    /// The source cursor hit a gap and was explicitly rebased.
    SourceGap,
    /// The feed could not currently express a covered source.
    SourceUnavailable,
    /// The feed/session/controller was unavailable or no longer live.
    SessionUnavailable,
}

/// Host-visible outcome for a feed load/drain command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FeedLoadStatus {
    /// Whether visible feed state changed and a snapshot emit was requested.
    pub changed: bool,
    /// Why the load/drain stopped.
    pub reason: FeedLoadStopReason,
}

impl FeedLoadStatus {
    #[must_use]
    pub const fn changed(reason: FeedLoadStopReason) -> Self {
        Self {
            changed: true,
            reason,
        }
    }

    #[must_use]
    pub const fn unchanged(reason: FeedLoadStopReason) -> Self {
        Self {
            changed: false,
            reason,
        }
    }

    #[must_use]
    pub const fn session_unavailable() -> Self {
        Self::unchanged(FeedLoadStopReason::SessionUnavailable)
    }

    #[must_use]
    pub const fn from_changed(changed: bool) -> Self {
        let reason = FeedLoadStopReason::WindowFilled;
        if changed {
            Self::changed(reason)
        } else {
            Self::unchanged(reason)
        }
    }
}

impl From<crate::DrainStop> for FeedLoadStopReason {
    fn from(stop: crate::DrainStop) -> Self {
        match stop {
            crate::DrainStop::PageFilled => Self::WindowFilled,
            crate::DrainStop::Exhausted => Self::SourceExhausted,
            crate::DrainStop::Gap { .. } => Self::SourceGap,
            crate::DrainStop::ScanBudget => Self::SourceScanBudgetReached,
        }
    }
}
