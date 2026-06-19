//! `nmp-nip68` - NIP-68 picture-first feed primitives.
//!
//! Scope is deliberately narrow and app-neutral:
//!
//! - decode kind:20 picture events into immutable [`PictureEventRecord`] values;
//! - apply NIP-68 image constraints to shared NIP-92 `imeta` metadata;
//! - build deterministic picture-post drafts that can be published through the
//!   existing `nmp.publish` / `PublishRaw` action path.
//!
//! This crate imports the lower `nmp-nip92-types` wire/type substrate, but no
//! other `nmp-nip*` protocol crate. It does not open feeds, rank posts, upload
//! media, choose relays, or carry app-specific nouns. Apps open
//! `{"kinds":[20]}` through `nmp_app_open_interest`, upload through a
//! capability/protocol crate such as `nmp-blossom`, and compose the resulting
//! image descriptors here. `nmp-core` gains zero picture-event nouns.

pub mod build;
pub mod decode;
pub mod imeta;
pub mod kinds;

pub use build::{PicturePost, PicturePostBuildError, PicturePostBuilder, PicturePostDraft};
pub use decode::{try_from_event, try_from_kernel_event, PictureEventRecord};
pub use imeta::{
    is_accepted_image_mime, parse_imeta_tag, ImageDimensions, ImageMeta, ImetaField,
    ACCEPTED_IMAGE_MIME_TYPES,
};
pub use kinds::KIND_PICTURE_EVENT;
