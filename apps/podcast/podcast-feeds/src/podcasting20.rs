// Podcasting 2.0 namespace extension walker stub.
// Parses <podcast:transcript>, <podcast:chapters>, <podcast:value>,
// <podcast:person>, <podcast:soundbite>, <podcast:locked>, <podcast:guid>.
// Reference: docs/design/podcast/podcast-feeds.md §A.2.

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Podcasting20Extensions {
    pub transcript: Option<TranscriptRef>,
    pub chapters: Option<ChaptersRef>,
    pub value: Option<ValueBlock>,
    pub persons: Vec<PersonRef>,
    pub soundbites: Vec<SoundbiteRef>,
    pub locked: Option<bool>,
    pub guid: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TranscriptRef {
    pub url: Url,
    pub mime: String,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChaptersRef {
    pub url: Url,
    pub mime: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ValueBlock {
    pub model: String,
    pub recipients: Vec<ValueRecipient>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ValueRecipient {
    pub name: String,
    pub address: String,
    pub split: u8,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PersonRef {
    pub name: String,
    pub role: Option<String>,
    pub group: Option<String>,
    pub href: Option<Url>,
    pub img: Option<Url>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SoundbiteRef {
    pub start_s: f64,
    pub duration_s: f64,
    pub title: Option<String>,
}
