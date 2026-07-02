use serde::{Deserialize, Serialize};

use crate::{
    DEFAULT_FEED_WINDOW_LIMIT, DEFAULT_PULL_SCAN_BUDGET, MAX_FEED_WINDOW_LIMIT,
    MAX_PULL_SCAN_BUDGET,
};

/// What a feed's visible window does when a perspective/source reset invalidates
/// current rows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub enum FeedWindowResetPolicy {
    /// Clear rows and return the viewport to the first declared page.
    #[default]
    ResetToInitial,
    /// Clear rows but keep the currently-visible viewport size.
    PreserveVisibleLimit,
}

/// (d) WINDOW — bounded viewport, paging, and source scan policy (D8).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FeedWindowPolicy {
    /// Initial visible row count after open or default reset. Clamped into
    /// `1..=MAX_FEED_WINDOW_LIMIT` by [`FeedWindowPolicy::initial_visible_limit`].
    pub initial_limit: usize,
    /// Visible rows added per successful `load_older`.
    #[serde(default = "default_feed_window_page_size")]
    pub page_size: usize,
    /// Maximum visible rows the viewport may grow to.
    #[serde(default = "default_feed_window_max_visible")]
    pub max_visible: usize,
    /// Target accepted source rows per pull drain. `0` follows `page_size`.
    #[serde(default)]
    pub source_page_size: usize,
    /// Maximum source log rows visited by one pull drain.
    #[serde(default = "default_feed_window_scan_budget")]
    pub source_scan_budget: usize,
    /// Reset/regrow behavior when a reactive source changes.
    #[serde(default)]
    pub reset: FeedWindowResetPolicy,
}

impl Default for FeedWindowPolicy {
    fn default() -> Self {
        Self {
            initial_limit: DEFAULT_FEED_WINDOW_LIMIT,
            page_size: DEFAULT_FEED_WINDOW_LIMIT,
            max_visible: MAX_FEED_WINDOW_LIMIT,
            source_page_size: DEFAULT_FEED_WINDOW_LIMIT,
            source_scan_budget: DEFAULT_PULL_SCAN_BUDGET,
            reset: FeedWindowResetPolicy::ResetToInitial,
        }
    }
}

impl FeedWindowPolicy {
    /// Build the common bounded visible-row policy.
    ///
    /// The same value is used for the initial page, visible regrow page, and
    /// accepted source-page target; scan budget and max-visible keep their
    /// bounded defaults.
    #[must_use]
    pub fn bounded(initial_limit: usize) -> Self {
        Self {
            initial_limit,
            page_size: initial_limit,
            source_page_size: initial_limit,
            ..Self::default()
        }
    }

    /// The initial visible limit clamped into the bounded range.
    #[must_use]
    pub fn initial_visible_limit(&self) -> usize {
        clamp_window_limit(self.initial_limit, DEFAULT_FEED_WINDOW_LIMIT).min(self.max_visible())
    }

    /// Scalar visible-row limit for callers that need only the initial bound.
    #[must_use]
    pub fn bounded_limit(&self) -> usize {
        self.initial_visible_limit()
    }

    /// Visible rows added per successful `load_older`.
    #[must_use]
    pub fn page_size(&self) -> usize {
        clamp_window_limit(self.page_size, DEFAULT_FEED_WINDOW_LIMIT)
    }

    /// Maximum visible rows the viewport may grow to.
    #[must_use]
    pub fn max_visible(&self) -> usize {
        clamp_window_limit(self.max_visible, MAX_FEED_WINDOW_LIMIT)
    }

    /// Target accepted source rows per pull drain.
    #[must_use]
    pub fn source_page_size(&self) -> usize {
        let fallback = self.page_size();
        clamp_window_limit(self.source_page_size, fallback)
    }

    /// Maximum source log rows visited by one pull drain.
    #[must_use]
    pub fn source_scan_budget(&self) -> usize {
        if self.source_scan_budget == 0 {
            DEFAULT_PULL_SCAN_BUDGET
        } else {
            self.source_scan_budget.clamp(1, MAX_PULL_SCAN_BUDGET)
        }
    }

    /// Return the next visible limit after one regrow step.
    #[must_use]
    pub fn next_visible_limit(&self, current: usize, total_rows: usize) -> Option<usize> {
        let max_visible = self.max_visible().min(total_rows);
        if current >= max_visible {
            return None;
        }
        Some((current + self.page_size()).min(max_visible))
    }

    /// Visible limit after a reset.
    #[must_use]
    pub fn reset_visible_limit(&self, current: usize) -> usize {
        match self.reset {
            FeedWindowResetPolicy::ResetToInitial => self.initial_visible_limit(),
            FeedWindowResetPolicy::PreserveVisibleLimit => current.max(1).min(self.max_visible()),
        }
    }
}

fn clamp_window_limit(value: usize, fallback: usize) -> usize {
    if value == 0 {
        fallback
    } else {
        value.min(MAX_FEED_WINDOW_LIMIT)
    }
}

const fn default_feed_window_page_size() -> usize {
    DEFAULT_FEED_WINDOW_LIMIT
}

const fn default_feed_window_max_visible() -> usize {
    MAX_FEED_WINDOW_LIMIT
}

const fn default_feed_window_scan_budget() -> usize {
    DEFAULT_PULL_SCAN_BUDGET
}
