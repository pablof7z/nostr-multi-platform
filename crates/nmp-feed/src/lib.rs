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
//!   targets, concept-owned counts, or previews; components and sibling modules
//!   own those dependencies.

mod admit;
mod custom_policy;
mod flat;
mod load_status;
mod pager;
mod params;
mod pull_controller;
mod registry;
mod root_indexed;
mod session;
mod spec;
pub mod typed_wire;
mod types;
mod window;
mod window_source;

pub use admit::AdmitExpr;
pub use custom_policy::{
    CustomAdmissionDef, CustomFeedPolicyRegistry, CustomOrderDef, CustomSourceDef,
};
pub use flat::{FlatFeed, FlatFeedItem, FlatFeedItemBuilder, FlatFeedMerge, FlatFeedPredicate};
pub use load_status::{FeedLoadStatus, FeedLoadStopReason};
pub use pager::{
    raw_to_kernel_event, DrainOutcome, DrainStop, FeedInterestShape, FeedPullPager,
    DEFAULT_PULL_PAGE_SIZE, DEFAULT_PULL_SCAN_BUDGET, MAX_PULL_SCAN_BUDGET,
};
pub use params::{
    CustomAdmissionId, CustomOrderId, CustomSourceId, FeedAdmission, FeedHandle,
    FeedItemProjection, FeedKey, FeedOrder, FeedParams, FeedScope, FeedSessionId, FeedShape,
    FeedSourceExpr, FeedWindowPolicy, FeedWindowResetPolicy, ListId, ProjectionKey, RelaySetId,
    TagTerm, WotRulesId, WotSeed,
};
pub use pull_controller::{
    ClosureInterestShape, ClosureInterestShapes, FeedAdvance, FeedApply, FeedInterestShapes,
    FeedReplace, FeedReset, PullFeedController, PullFn,
};
pub use registry::{new_feed_registry_slot, FeedController, FeedRegistry, FeedRegistrySlot};
pub use root_indexed::{
    admit_all_roots, AttributionAuthors, AttributionPayload, CardAuthors, CardBuilder, EventGate,
    EventLookup, FeedAuthorRefs, FollowPredicate, RootAdmission, RootCard, RootFeedSnapshot,
    RootIndexedFeed, MAX_ATTRIBUTION_PER_ROOT,
};
pub use session::{FeedSessionBuild, FeedSessionRegistry, TeardownAction};
pub use spec::{feed, source, FeedSpec, FeedSpecError};
pub use typed_wire::{
    decode_feed_window, encode_feed_window, FeedWindowWire, FEED_WINDOW_FILE_IDENTIFIER,
    FEED_WINDOW_SCHEMA_ID, FEED_WINDOW_SCHEMA_VERSION,
};
pub use types::{
    FeedBlock, FeedCard, FeedCardStore, FeedCursor, FeedPage, FeedRequest, FeedWindowMetrics,
    FeedWindowState, DEFAULT_FEED_WINDOW_LIMIT, MAX_FEED_WINDOW_LIMIT,
};
pub use window::{block_cursor, cards_for_blocks, page_for_request, sorted_blocks};
pub use window_source::FeedWindowSource;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
