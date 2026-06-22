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
//! | 10001+    | Other lists      | NIP-51 | Post-v1 unless named above |
//!
//! # Architecture
//!
//! The crate exposes [`MuteListProjection`], which is both a
//! [`nmp_core::KernelEventObserver`] (the write side — ingest kind:10000
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
//! The substrate-generic [`SuppressionLookup`] trait lives in `nmp-core` so
//! `nmp-nip01`'s `ModularTimelineProjection` can depend on it without creating
//! a `nmp-nip01 → nmp-nip51` edge (which would be a Layer-4 sibling
//! dependency, forbidden by the crate-boundary spec). At composition time the
//! host wires:
//!
//! ```text
//! let mute = Arc::new(MuteListProjection::new(Arc::clone(&active_pubkey_slot)));
//! app.register_event_observer(Arc::clone(&mute) as Arc<dyn KernelEventObserver>);
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
//! in a ranked feed). The duplication of the kind:10000 `p`-tag parse is an
//! acknowledged overlap tracked as GitHub issue #964. Consolidating both onto
//! `nmp-nip51`'s decode would
//! require `nmp-wot` to depend on `nmp-nip51` — a legal Layer-4 sibling edge
//! per the spec. That consolidation is a future clean-up step, not v1 scope.

pub mod bookmarks;
pub mod interests;
pub mod people_list;
pub mod projection;
pub mod search_relays;
pub mod wire;

pub use interests::{
    active_bookmark_list_interest, active_bookmark_list_interest_id, active_mute_list_interest,
    active_mute_list_interest_id, active_search_relay_list_interest,
    active_search_relay_list_interest_id,
};


pub use bookmarks::{
    build_bookmark_list_event, register_bookmark_actions, AddBookmarkAction, BookmarkItem,
    BookmarkListMetadata, BookmarkListProjection, BookmarkListSnapshot, BookmarkUpdateInput,
    RemoveBookmarkAction,
};
pub use people_list::{PeopleListProjection, PeopleListSnapshot};
pub use projection::{MuteListProjection, MuteListSnapshot};
pub use search_relays::{SearchRelayListProjection, SearchRelayListSnapshot};
pub use wire::mute_list_fb::{
    decode_mute_list, encode_mute_list, MUTE_LIST_FILE_IDENTIFIER, MUTE_LIST_SCHEMA_ID,
    MUTE_LIST_SCHEMA_VERSION,
};
