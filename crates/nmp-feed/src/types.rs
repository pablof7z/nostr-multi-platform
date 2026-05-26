use nmp_core::substrate::BoundedMessageMap;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

pub const DEFAULT_FEED_WINDOW_LIMIT: usize = 80;
pub const MAX_FEED_WINDOW_LIMIT: usize = 500;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeedCursor {
    pub created_at: u64,
    pub id: String,
}

impl FeedCursor {
    #[must_use]
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.created_at > other.created_at
            || (self.created_at == other.created_at && self.id > other.id)
    }

    #[must_use]
    pub fn is_older_than(&self, other: &Self) -> bool {
        self.created_at < other.created_at
            || (self.created_at == other.created_at && self.id < other.id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeedRequest {
    #[serde(default = "default_feed_window_limit")]
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<FeedCursor>,
}

impl Default for FeedRequest {
    fn default() -> Self {
        Self {
            limit: DEFAULT_FEED_WINDOW_LIMIT,
            cursor: None,
        }
    }
}

impl FeedRequest {
    #[must_use]
    pub fn newest(limit: usize) -> Self {
        Self {
            limit,
            cursor: None,
        }
    }

    #[must_use]
    pub fn bounded_limit(&self) -> usize {
        if self.limit == 0 {
            DEFAULT_FEED_WINDOW_LIMIT
        } else {
            self.limit.min(MAX_FEED_WINDOW_LIMIT)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeedPage {
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<FeedCursor>,
    pub has_more: bool,
    pub total_blocks: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeedWindowMetrics {
    pub make_window_us: u64,
}

#[derive(Clone, Debug, Default)]
pub struct FeedWindowState {
    pub(crate) oldest_visible: Option<FeedCursor>,
}

pub trait FeedBlock: Clone {
    fn feed_event_ids(&self) -> Vec<String>;
}

impl FeedBlock for TimelineBlock {
    fn feed_event_ids(&self) -> Vec<String> {
        match self {
            Self::Standalone(id) => vec![id.clone()],
            Self::Module { events, .. } => events.clone(),
        }
    }
}

pub trait FeedCard: Clone {
    fn feed_created_at(&self) -> u64;
    fn feed_event_refs(&self) -> Vec<String>;
}

pub trait FeedCardStore<C: FeedCard> {
    fn feed_card(&self, id: &str) -> Option<&C>;
}

impl<C: FeedCard> FeedCardStore<C> for BoundedMessageMap<String, C> {
    fn feed_card(&self, id: &str) -> Option<&C> {
        self.get(id)
    }
}

const fn default_feed_window_limit() -> usize {
    DEFAULT_FEED_WINDOW_LIMIT
}
