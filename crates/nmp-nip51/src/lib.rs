//! `nmp-nip51` — NIP-51 list projections for the NMP substrate.
//!
//! # Scope
//!
//! NIP-51 specifies a family of "curated set" event kinds. This crate owns
//! parsing and projection of the NIP-51 list events that NMP consumes. It does
//! not own planner/router policy; it exposes facts for substrate owners to use.
//!
//! | Wire kind | Name             | NIP    | Status   |
//! |-----------|------------------|--------|----------|
//! | 10000     | Public mute      | NIP-51 | Shipped  |
//! | 10003     | Global bookmarks | NIP-51 | Shipped as raw projection + safe RMW builders |
//! | 10007     | Search relays    | NIP-51 | Shipped as active-account facts |
//! | 30003     | Bookmark sets    | NIP-51 | Shipped as raw projection + safe RMW builders |
//! | 30004     | Curation sets    | NIP-51 | Shipped as raw projection + safe RMW builders |
//! | 39701     | Web bookmarks    | NIP-B0 | Shipped as raw projection + safe publish builder |
//! | 10001+    | Other lists      | NIP-51 | Post-v1 unless named above |
//!
//! # Architecture
//!
//! The crate exposes [`MuteListProjection`], which is both a
//! [`nmp_core::ObservedProjectionSink`] (the write side — ingest kind:10000
//! events) and a [`nmp_core::substrate::SuppressionLookup`] implementation
//! (the read side — answer "is this author/event muted?" queries from the
//! timeline projection).
//!
//! It also exposes [`SearchRelayListProjection`], a kind:10007 active-account
//! relay-list projection. Search query semantics and ranking stay with the
//! owning search module; this crate only parses `["relay", <url>]` facts.
//!
//! It also exposes [`BookmarkListProjection`] plus
//! [`AddBookmarkAction`] / [`RemoveBookmarkAction`] for the active account's
//! kind:10003 global bookmark list. The projection stays raw: event ids,
//! address coordinates, URLs, hashtags, and NIP-51 metadata. App-specific
//! vault organization, privacy language, and UI flows stay in app crates.
//!
//! It also exposes [`BookmarkSetsProjection`] for kind:30003 bookmark sets and
//! kind:30004 curation sets, plus [`WebBookmarksProjection`] for NIP-B0
//! kind:39701 web bookmarks. These projections are explicit-author read
//! surfaces: products decide which authors to subscribe to, while this crate
//! only parses the protocol facts it receives.
//!
//! The substrate-generic [`SuppressionLookup`] trait lives in `nmp-core` so
//! `nmp-nip01`'s `ModularTimelineProjection` can depend on it without creating
//! a `nmp-nip01 → nmp-nip51` edge (which would be a Layer-4 sibling
//! dependency, forbidden by the crate-boundary spec). At composition time the
//! host wires:
//!
//! ```text
//! let mute = Arc::new(MuteListProjection::new(Arc::clone(&active_pubkey_slot)));
//! app.open_observed_projection(ObservedProjection::from_kinds(
//!     Arc::clone(&mute) as Arc<dyn ObservedProjectionSink>,
//!     "nmp.nip51.mutes",
//!     0,
//!     [KIND_MUTE_LIST],
//!     128,
//! ));
//! timeline.set_suppression(Arc::clone(&mute) as Arc<dyn SuppressionLookup>);
//! ```
//!
//! # D0 — namespace hygiene
//!
//! `nmp-core` sees no NIP-51 nouns. The substrate trait is `SuppressionLookup`
//! with methods `is_suppressed_author` / `is_suppressed_event`. The NIP-51
//! kind number (10000) is a constant local to this crate.
//!
//! # Public tags only
//!
//! NIP-51 allows private mutes in the NIP-44 encrypted `content` field. This
//! crate only parses public `p` and `e` tags. Private-mute decryption requires
//! the active signer and is post-v1.
//!
//! # Relationship to `nmp-wot`
//!
//! `nmp-wot` also ingests kind:10000 events to populate its `WotGraph` for
//! follow-graph scoring. The two crates serve different consumers:
//! `nmp-nip51` serves the **timeline suppression** (hard mute — hide the card
//! entirely), `nmp-wot` serves **trust scoring** (soft signal — deprioritize
//! in a ranked feed). Both consumers previously maintained independent
//! kind:10000 `p`-tag scanners (GitHub issue #964 — acknowledged overlap).
//!
//! That duplication is now **eliminated**: [`mute_pubkeys_from_tags`] is the
//! single canonical parser for kind:10000 `p` tags. `nmp-wot` takes a legal
//! Layer-4 sibling dependency on `nmp-nip51` and calls this function instead
//! of its own internal scanner, while `MuteListProjection::on_kernel_event`
//! also drives through the same function. The distinction between **hard
//! suppression** (this crate) and **soft trust scoring** (`nmp-wot`) is
//! preserved — only the tag-parse step is shared.

pub mod bookmark_sets;
pub mod bookmarks;
pub mod interests;
pub mod people_list;
pub mod projection;
pub mod runtime;
pub mod search_fallback;
pub mod search_relays;
pub mod web_bookmarks;
pub mod wire;

pub use interests::{
    active_bookmark_list_identity, active_bookmark_list_interest, active_bookmark_list_interest_id,
    active_mute_list_identity, active_mute_list_interest, active_mute_list_interest_id,
    bookmark_sets_identity, bookmark_sets_interest, bookmark_sets_interest_id,
    web_bookmarks_identity, web_bookmarks_interest, web_bookmarks_interest_id,
};

pub use bookmark_sets::{
    build_bookmark_set_event, register_bookmark_set_actions, AddBookmarkSetItemAction,
    BookmarkSetKind, BookmarkSetSnapshot, BookmarkSetUpdateInput, BookmarkSetsProjection,
    BookmarkSetsSnapshot, RemoveBookmarkSetItemAction,
};
pub use bookmarks::{
    build_bookmark_list_event, register_bookmark_actions, AddBookmarkAction, BookmarkItem,
    BookmarkListMetadata, BookmarkListProjection, BookmarkListSnapshot, BookmarkUpdateInput,
    RemoveBookmarkAction,
};
pub use people_list::{PeopleListProjection, PeopleListSnapshot};
pub use projection::{
    mute_pubkeys_from_tags, MuteListProjection, MuteListSnapshot, ACTIVE_MUTE_LIST_PUBKEY_SOURCE_ID,
};
pub use runtime::{
    register_bookmark_runtime, register_bookmark_set_runtime, register_mute_runtime,
    register_search_relay_runtime_with_fallbacks, register_web_bookmark_runtime,
};
pub use search_fallback::effective_search_relays;
pub use search_relays::{SearchRelayListProjection, SearchRelayListSnapshot};
pub use web_bookmarks::{
    build_web_bookmark_event, register_web_bookmark_actions, PublishWebBookmarkAction,
    PublishWebBookmarkInput, WebBookmarkDraft, WebBookmarkSnapshot, WebBookmarksProjection,
    WebBookmarksSnapshot,
};
pub use wire::bookmark_list_fb::{
    decode_bookmark_list, encode_bookmark_list, BOOKMARK_LIST_FILE_IDENTIFIER,
    BOOKMARK_LIST_SCHEMA_ID, BOOKMARK_LIST_SCHEMA_VERSION,
};
pub use wire::mute_list_fb::{
    decode_mute_list, encode_mute_list, MUTE_LIST_FILE_IDENTIFIER, MUTE_LIST_SCHEMA_ID,
    MUTE_LIST_SCHEMA_VERSION,
};
