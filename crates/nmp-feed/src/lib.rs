//! Reusable Nostr feed viewport primitives.
//!
//! Protocol projections provide feed blocks and render cards; this crate owns
//! stable cursor ordering, bounded viewport state, transitive card inclusion,
//! and generic feed-controller registration.
//!
//! Doctrine map:
//! - D0: protocol/app crates supply admission predicates, card builders, and
//!   merge policy. This crate owns mechanics only and names no app primary-kind
//!   policy.
//! - D5: feed state and emitted snapshots are bounded by the visible window.
//! - D11: feed engines never claim secondary data such as profiles, missing
//!   targets, relation counts, or previews; components and sibling modules own
//!   those dependencies.

mod flat;
mod pager;
mod params;
mod pull_controller;
mod registry;
mod root_indexed;
mod session;
pub mod typed_wire;
mod types;
mod window;

pub use flat::{FlatFeed, FlatFeedItem, FlatFeedItemBuilder, FlatFeedMerge, FlatFeedPredicate};
pub use pager::{
    raw_to_kernel_event, DrainOutcome, DrainStop, FeedInterestShape, FeedPullPager,
    DEFAULT_PULL_PAGE_SIZE, DEFAULT_PULL_SCAN_BUDGET, MAX_PULL_SCAN_BUDGET,
};
pub use params::{
    validate_primary_kinds, CustomPerspectiveId, FeedAdmission, FeedHandle, FeedParams,
    FeedParamsError, FeedRanking, FeedScope, FeedSessionId, FeedWindow, ListId, ProjectionKey,
    PubkeySetExpr, RelaySetId, TagTerm, WotRulesId, WotSeed, KIND_DELETE,
};
pub use pull_controller::{
    ClosureInterestShape, FeedAdvance, FeedApply, FeedReplace, FeedReset, PullFeedController,
    PullFn,
};
pub use registry::{new_feed_registry_slot, FeedController, FeedRegistry, FeedRegistrySlot};
pub use session::{FeedSessionBuild, FeedSessionRegistry, TeardownAction};
pub use root_indexed::{
    AttributionPayload, CardBuilder, EventGate, EventLookup, FollowPredicate, RootCard,
    RootFeedSnapshot, RootIndexedFeed, MAX_ATTRIBUTION_PER_ROOT,
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
