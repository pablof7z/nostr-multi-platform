//! `nmp-nip68` - NIP-68 picture-first feed primitives.
//!
//! Scope is deliberately narrow and app-neutral:
//!
//! - decode kind:20 picture events into immutable [`PictureEventRecord`] values;
//! - apply NIP-68 image constraints to shared NIP-92 `imeta` metadata;
//! - build deterministic picture-post drafts that can be published through the
//!   existing `nmp.publish` / `PublishRaw` action path;
//! - bind kind:20 plus derived kind:16 repost wrappers to generic `nmp-feed`
//!   mechanics without owning app card policy.
//!
//! This crate imports the lower `nmp-nip92-types` wire/type substrate and
//! NIP-18 repost decoding for the feed adapter. It does not rank posts, upload
//! media, choose relays, or carry app-specific nouns. Feed apps declare primary
//! kind `20`; this crate derives and admits kind:16 repost wrappers at the
//! protocol layer, while the app supplies the perspective predicate and renders
//! its own photo cards. `nmp-core` gains zero picture-event nouns.

pub mod build;
pub mod decode;
pub mod feed;
pub mod imeta;
pub mod kinds;

pub use build::{PicturePost, PicturePostBuildError, PicturePostBuilder, PicturePostDraft};
pub use decode::{try_from_event, try_from_kernel_event, PictureEventRecord};
pub use feed::{
    picture_acquisition_kinds, picture_feed_observer, picture_feed_predicate, picture_feed_shape,
    PictureFeed, PictureFeedEntry, PictureFeedObserver, PictureFeedPredicate,
    PictureRepostAttribution,
};
pub use imeta::{
    is_accepted_image_mime, parse_imeta_tag, ImetaField, MediaDimensions, MediaMeta,
    ACCEPTED_IMAGE_MIME_TYPES,
};
pub use kinds::KIND_PICTURE_EVENT;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
