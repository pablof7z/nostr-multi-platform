//! NIP-68 image constraints over shared NIP-92 `imeta` metadata.

pub use nmp_nip92_types::{ImetaField, MediaDimensions, MediaMeta};

/// MIME types accepted by NIP-68 picture events.
pub const ACCEPTED_IMAGE_MIME_TYPES: &[&str] = &[
    "image/apng",
    "image/avif",
    "image/gif",
    "image/jpeg",
    "image/png",
    "image/webp",
];

#[must_use]
pub fn is_accepted_image_mime(mime: &str) -> bool {
    ACCEPTED_IMAGE_MIME_TYPES.contains(&mime)
}

#[must_use]
pub fn image_has_supported_mime(image: &MediaMeta) -> bool {
    image.mime.as_deref().map_or(true, is_accepted_image_mime)
}

#[must_use]
pub fn parse_imeta_tag(tag: &[String]) -> Option<MediaMeta> {
    let image = nmp_nip92_types::parse_imeta_tag(tag)?;
    if image_has_supported_mime(&image) {
        Some(image)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "imeta_tests.rs"]
mod tests;
