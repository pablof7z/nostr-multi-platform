//! Builder half - deterministic kind:20 picture-post blueprints.

use std::collections::BTreeSet;

use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::imeta::MediaMeta;
use crate::kinds::KIND_PICTURE_EVENT;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PicturePostBuildError {
    MissingImage,
    EmptyImageUrl,
    IncompleteMediaMetadata { url: String },
    UnsupportedMime { mime: String },
}

impl core::fmt::Display for PicturePostBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingImage => write!(f, "NIP-68 picture event requires at least one image"),
            Self::EmptyImageUrl => write!(f, "NIP-68 image metadata requires a non-empty url"),
            Self::IncompleteMediaMetadata { url } => {
                write!(
                    f,
                    "NIP-68 imeta for {url} requires url plus at least one metadata field"
                )
            }
            Self::UnsupportedMime { mime } => {
                write!(f, "NIP-68 image MIME type is not accepted: {mime}")
            }
        }
    }
}

impl std::error::Error for PicturePostBuildError {}

pub struct PicturePost;

impl PicturePost {
    #[must_use]
    pub fn new(image: MediaMeta) -> PicturePostBuilder {
        PicturePostBuilder::empty().image(image)
    }

    #[must_use]
    pub fn with_images(images: Vec<MediaMeta>) -> PicturePostBuilder {
        PicturePostBuilder {
            images,
            ..PicturePostBuilder::empty()
        }
    }
}

#[derive(Clone, Debug)]
pub struct PicturePostBuilder {
    images: Vec<MediaMeta>,
    title: Option<String>,
    content: String,
    content_warning: Option<String>,
    tagged_pubkeys: Vec<String>,
    hashtags: Vec<String>,
    location: Option<String>,
    geohash: Option<String>,
    languages: Vec<(String, String)>,
    extra_tags: Vec<Vec<String>>,
}

impl PicturePostBuilder {
    fn empty() -> Self {
        Self {
            images: Vec::new(),
            title: None,
            content: String::new(),
            content_warning: None,
            tagged_pubkeys: Vec::new(),
            hashtags: Vec::new(),
            location: None,
            geohash: None,
            languages: Vec::new(),
            extra_tags: Vec::new(),
        }
    }

    #[must_use]
    pub fn image(mut self, image: MediaMeta) -> Self {
        self.images.push(image);
        self
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = non_empty(title.into());
        self
    }

    #[must_use]
    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    #[must_use]
    pub fn content_warning(mut self, reason: impl Into<String>) -> Self {
        self.content_warning = non_empty(reason.into());
        self
    }

    #[must_use]
    pub fn tagged_pubkey(mut self, pubkey: impl Into<String>) -> Self {
        push_non_empty(&mut self.tagged_pubkeys, pubkey.into());
        self
    }

    #[must_use]
    pub fn hashtag(mut self, hashtag: impl Into<String>) -> Self {
        let hashtag = hashtag.into().trim_start_matches('#').to_string();
        push_non_empty(&mut self.hashtags, hashtag);
        self
    }

    #[must_use]
    pub fn location(mut self, location: impl Into<String>) -> Self {
        self.location = non_empty(location.into());
        self
    }

    #[must_use]
    pub fn geohash(mut self, geohash: impl Into<String>) -> Self {
        self.geohash = non_empty(geohash.into());
        self
    }

    #[must_use]
    pub fn language(mut self, code: impl Into<String>, namespace: impl Into<String>) -> Self {
        let code = code.into();
        let namespace = namespace.into();
        if !code.is_empty() && !namespace.is_empty() {
            self.languages.push((code, namespace));
        }
        self
    }

    #[must_use]
    pub fn extra_tag(mut self, tag: Vec<String>) -> Self {
        if !tag.is_empty() {
            self.extra_tags.push(tag);
        }
        self
    }

    /// Build protocol fields without signer, clock, or relay policy.
    ///
    /// # Errors
    ///
    /// Returns a [`PicturePostBuildError`] when the image set cannot produce
    /// protocol-valid NIP-92 `imeta` tags.
    pub fn build(self) -> Result<PicturePostDraft, PicturePostBuildError> {
        if self.images.is_empty() {
            return Err(PicturePostBuildError::MissingImage);
        }
        for image in &self.images {
            validate_image(image)?;
        }

        let mut tags = vec![vec!["title".to_string(), self.title.unwrap_or_default()]];
        for image in &self.images {
            tags.push(image.imeta_tag());
        }
        if let Some(reason) = self.content_warning {
            tags.push(vec!["content-warning".to_string(), reason]);
        }
        for pubkey in self.tagged_pubkeys {
            tags.push(vec!["p".to_string(), pubkey]);
        }

        for mime in ordered_image_mimes(&self.images) {
            tags.push(vec!["m".to_string(), mime]);
        }
        for hash in ordered_image_hashes(&self.images) {
            tags.push(vec!["x".to_string(), hash]);
        }
        for hashtag in self.hashtags {
            tags.push(vec!["t".to_string(), hashtag]);
        }
        if let Some(location) = self.location {
            tags.push(vec!["location".to_string(), location]);
        }
        if let Some(geohash) = self.geohash {
            tags.push(vec!["g".to_string(), geohash]);
        }
        for (code, namespace) in self.languages {
            tags.push(vec!["L".to_string(), namespace.clone()]);
            tags.push(vec!["l".to_string(), code, namespace]);
        }
        tags.extend(self.extra_tags);

        Ok(PicturePostDraft {
            kind: KIND_PICTURE_EVENT,
            tags,
            content: self.content,
        })
    }

    pub fn build_unsigned(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, PicturePostBuildError> {
        self.build()
            .map(|draft| draft.into_unsigned(author, created_at))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PicturePostDraft {
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

impl PicturePostDraft {
    #[must_use]
    pub fn into_unsigned(self, author: impl Into<String>, created_at: u64) -> UnsignedEvent {
        UnsignedEvent {
            pubkey: author.into(),
            kind: self.kind,
            tags: self.tags,
            content: self.content,
            created_at,
        }
    }
}

fn validate_image(image: &MediaMeta) -> Result<(), PicturePostBuildError> {
    if image.url.is_empty() {
        return Err(PicturePostBuildError::EmptyImageUrl);
    }
    if image.metadata_field_count() == 0 {
        return Err(PicturePostBuildError::IncompleteMediaMetadata {
            url: image.url.clone(),
        });
    }
    if let Some(mime) = &image.mime {
        if !crate::imeta::image_has_supported_mime(image) {
            return Err(PicturePostBuildError::UnsupportedMime { mime: mime.clone() });
        }
    }
    Ok(())
}

fn ordered_image_mimes(images: &[MediaMeta]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    images
        .iter()
        .filter_map(|image| image.mime.clone())
        .filter(|mime| seen.insert(mime.clone()))
        .collect()
}

fn ordered_image_hashes(images: &[MediaMeta]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    images
        .iter()
        .filter_map(|image| image.sha256.clone())
        .filter(|hash| seen.insert(hash.clone()))
        .collect()
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn push_non_empty(values: &mut Vec<String>, value: String) {
    if !value.is_empty() {
        values.push(value);
    }
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
