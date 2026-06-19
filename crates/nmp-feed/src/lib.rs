//! Reusable Nostr feed viewport primitives.
//!
//! Protocol projections provide feed blocks and render cards; this crate owns
//! stable cursor ordering, bounded viewport state, transitive card inclusion,
//! and generic feed-controller registration.

mod pager;
mod registry;
pub mod typed_wire;
mod root_indexed;
mod types;
mod window;

pub use pager::{
    raw_to_kernel_event, DrainOutcome, DrainStop, FeedInterestShape, FeedPullPager,
    DEFAULT_PULL_PAGE_SIZE, DEFAULT_PULL_SCAN_BUDGET, MAX_PULL_SCAN_BUDGET,
};
pub use registry::{new_feed_registry_slot, FeedController, FeedRegistry, FeedRegistrySlot};
pub use root_indexed::{
    AttributionPayload, CardBuilder, ClaimRequest, ClaimSink, EventGate, EventLookup,
    FollowPredicate, ProfileDetector, RootCard, RootFeedSnapshot, RootIndexedFeed,
    MAX_ATTRIBUTION_PER_ROOT,
};
pub use typed_wire::{
    decode_feed_window, encode_feed_window, FeedWindowWire, FEED_WINDOW_FILE_IDENTIFIER,
    FEED_WINDOW_SCHEMA_ID, FEED_WINDOW_SCHEMA_VERSION,
};
pub use types::{
    FeedBlock, FeedCard, FeedCardStore, FeedCursor, FeedPage, FeedRequest, FeedWindowMetrics,
    FeedWindowState, DEFAULT_FEED_WINDOW_LIMIT, MAX_FEED_WINDOW_LIMIT,
};
pub use window::{block_cursor, cards_for_blocks, page_for_request, sorted_blocks};
