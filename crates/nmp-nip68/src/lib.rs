//! `nmp-nip68` - NIP-68 picture-first feed primitives.
//!
//! Scope is deliberately narrow and app-neutral:
//!
//! - decode kind:20 picture events into immutable [`PictureEventRecord`] values;
//! - parse and build NIP-92 `imeta` image metadata tags;
//! - build deterministic picture-post drafts that can be published through the
//!   existing `nmp.publish` / `PublishRaw` action path.
//!
//! This crate does not open feeds, rank posts, upload media, choose relays, or
//! carry app-specific nouns. Apps open `{"kinds":[20]}` through
//! `nmp_app_open_interest`, upload through a capability/protocol crate such as
//! `nmp-blossom`, and compose the resulting image descriptors here.

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
