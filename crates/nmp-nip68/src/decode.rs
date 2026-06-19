//! Decoder half - immutable records from NIP-68 kind:20 picture events.

use nmp_core::store::StoredEvent;
use nmp_core::substrate::KernelEvent;
use serde::{Deserialize, Serialize};

use crate::imeta::{parse_imeta_tag, ImageMeta};
use crate::kinds::KIND_PICTURE_EVENT;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PictureEventRecord {
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub content: String,
    pub images: Vec<ImageMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_warning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tagged_pubkeys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashtags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geohash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub language_tags: Vec<Vec<String>>,
}

#[must_use]
pub fn try_from_event(event: &StoredEvent) -> Option<PictureEventRecord> {
    let raw = event.raw.as_ref();
    decode_borrowed(
        &raw.id,
        &raw.pubkey,
        raw.kind,
        raw.created_at,
        &raw.tags,
        &raw.content,
    )
}

#[must_use]
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<PictureEventRecord> {
    decode_borrowed(
        &event.id,
        &event.author,
        event.kind,
        event.created_at,
        &event.tags,
        &event.content,
    )
}

fn decode_borrowed(
    id: &str,
    author: &str,
    kind: u32,
    created_at: u64,
    tags: &[Vec<String>],
    content: &str,
) -> Option<PictureEventRecord> {
    if kind != KIND_PICTURE_EVENT {
        return None;
    }

    let images: Vec<ImageMeta> = tags.iter().filter_map(|tag| parse_imeta_tag(tag)).collect();
    if images.is_empty() {
        return None;
    }

    Some(PictureEventRecord {
        event_id: id.to_string(),
        author: author.to_string(),
        created_at,
        title: first_tag_value(tags, "title").map(str::to_string),
        content: content.to_string(),
        images,
        content_warning: first_tag_value(tags, "content-warning").map(str::to_string),
        tagged_pubkeys: tag_values(tags, "p"),
        hashtags: tag_values(tags, "t"),
        media_types: tag_values(tags, "m"),
        hashes: tag_values(tags, "x"),
        location: first_tag_value(tags, "location").map(str::to_string),
        geohash: first_tag_value(tags, "g").map(str::to_string),
        language_tags: tags
            .iter()
            .filter(|tag| tag.first().is_some_and(|key| key == "L" || key == "l"))
            .cloned()
            .collect(),
    })
}

fn first_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == key) {
            tag.get(1).filter(|v| !v.is_empty()).map(String::as_str)
        } else {
            None
        }
    })
}

fn tag_values(tags: &[Vec<String>], key: &str) -> Vec<String> {
    tags.iter()
        .filter_map(|tag| {
            if tag.first().is_some_and(|name| name == key) {
                tag.get(1).filter(|v| !v.is_empty()).cloned()
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
