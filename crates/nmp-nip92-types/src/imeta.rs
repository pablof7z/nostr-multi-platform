//! NIP-92 `imeta` parsing/building for generic media metadata.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MediaDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImetaField {
    pub key: String,
    pub value: String,
}

impl ImetaField {
    #[must_use]
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Parsed metadata for one NIP-92 media attachment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MediaMeta {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbhash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<MediaDimensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<ImetaField>,
}

impl MediaMeta {
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            sha256: None,
            mime: None,
            thumbhash: None,
            blurhash: None,
            dimensions: None,
            alt: None,
            fallbacks: Vec::new(),
            annotations: Vec::new(),
            extra: Vec::new(),
        }
    }

    #[must_use]
    pub fn sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sha256 = non_empty(sha256.into());
        self
    }

    #[must_use]
    pub fn mime(mut self, mime: impl Into<String>) -> Self {
        self.mime = non_empty(mime.into());
        self
    }

    #[must_use]
    pub fn thumbhash(mut self, thumbhash: impl Into<String>) -> Self {
        self.thumbhash = non_empty(thumbhash.into());
        self
    }

    #[must_use]
    pub fn blurhash(mut self, blurhash: impl Into<String>) -> Self {
        self.blurhash = non_empty(blurhash.into());
        self
    }

    #[must_use]
    pub fn dimensions(mut self, width: u32, height: u32) -> Self {
        self.dimensions = if width == 0 || height == 0 {
            None
        } else {
            Some(MediaDimensions { width, height })
        };
        self
    }

    #[must_use]
    pub fn alt(mut self, alt: impl Into<String>) -> Self {
        self.alt = non_empty(alt.into());
        self
    }

    #[must_use]
    pub fn fallback(mut self, fallback: impl Into<String>) -> Self {
        push_non_empty(&mut self.fallbacks, fallback.into());
        self
    }

    #[must_use]
    pub fn annotation(mut self, annotation: impl Into<String>) -> Self {
        push_non_empty(&mut self.annotations, annotation.into());
        self
    }

    #[must_use]
    pub fn extra_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let value = value.into();
        if !key.trim().is_empty() && !value.is_empty() {
            self.extra.push(ImetaField::new(key, value));
        }
        self
    }

    /// Build the variadic `["imeta", "url ...", ...]` tag.
    #[must_use]
    pub fn imeta_tag(&self) -> Vec<String> {
        let mut tag = Vec::with_capacity(8 + self.fallbacks.len() + self.annotations.len());
        tag.push("imeta".to_string());
        tag.push(format!("url {}", self.url));
        if let Some(sha256) = &self.sha256 {
            tag.push(format!("x {sha256}"));
        }
        if let Some(mime) = &self.mime {
            tag.push(format!("m {mime}"));
        }
        if let Some(thumbhash) = &self.thumbhash {
            tag.push(format!("thumbhash {thumbhash}"));
        }
        if let Some(blurhash) = &self.blurhash {
            tag.push(format!("blurhash {blurhash}"));
        }
        if let Some(dim) = &self.dimensions {
            tag.push(format!("dim {}x{}", dim.width, dim.height));
        }
        if let Some(alt) = &self.alt {
            tag.push(format!("alt {alt}"));
        }
        for fallback in &self.fallbacks {
            tag.push(format!("fallback {fallback}"));
        }
        for annotation in &self.annotations {
            tag.push(format!("annotate-user {annotation}"));
        }
        for field in &self.extra {
            tag.push(format!("{} {}", field.key, field.value));
        }
        tag
    }

    #[must_use]
    pub fn metadata_field_count(&self) -> usize {
        usize::from(self.sha256.is_some())
            + usize::from(self.mime.is_some())
            + usize::from(self.thumbhash.is_some())
            + usize::from(self.blurhash.is_some())
            + usize::from(self.dimensions.is_some())
            + usize::from(self.alt.is_some())
            + self.fallbacks.len()
            + self.annotations.len()
            + self.extra.len()
    }
}

#[must_use]
pub fn parse_imeta_tag(tag: &[String]) -> Option<MediaMeta> {
    if tag.first().map_or(true, |name| name != "imeta") {
        return None;
    }

    let mut media: Option<MediaMeta> = None;
    let mut pending: Vec<ImetaField> = Vec::new();

    for part in tag.iter().skip(1) {
        let Some((key, value)) = part.split_once(' ') else {
            continue;
        };
        if key.trim().is_empty() || value.is_empty() {
            continue;
        }

        if key == "url" {
            if media.is_none() {
                media = Some(MediaMeta::new(value.to_string()));
                if let Some(meta) = &mut media {
                    meta.extra.append(&mut pending);
                }
            } else if let Some(meta) = &mut media {
                meta.extra.push(ImetaField::new(key, value));
            }
            continue;
        }

        if let Some(meta) = &mut media {
            apply_field(meta, key, value);
        } else {
            pending.push(ImetaField::new(key, value));
        }
    }

    let media = media?;
    if media.metadata_field_count() == 0 {
        return None;
    }
    Some(media)
}

fn apply_field(media: &mut MediaMeta, key: &str, value: &str) {
    match key {
        "x" if media.sha256.is_none() => media.sha256 = Some(value.to_string()),
        "m" if media.mime.is_none() => media.mime = Some(value.to_string()),
        "thumbhash" if media.thumbhash.is_none() => media.thumbhash = Some(value.to_string()),
        "blurhash" if media.blurhash.is_none() => media.blurhash = Some(value.to_string()),
        "dim" if media.dimensions.is_none() => {
            if let Some(dimensions) = parse_dimensions(value) {
                media.dimensions = Some(dimensions);
            } else {
                media.extra.push(ImetaField::new(key, value));
            }
        }
        "alt" if media.alt.is_none() => media.alt = Some(value.to_string()),
        "fallback" => media.fallbacks.push(value.to_string()),
        "annotate-user" => media.annotations.push(value.to_string()),
        _ => media.extra.push(ImetaField::new(key, value)),
    }
}

fn parse_dimensions(raw: &str) -> Option<MediaDimensions> {
    let (w, h) = raw.split_once('x')?;
    let width = w.parse::<u32>().ok()?;
    let height = h.parse::<u32>().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(MediaDimensions { width, height })
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
