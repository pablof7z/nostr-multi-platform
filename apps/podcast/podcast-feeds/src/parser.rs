// Feed parser stub.
// Full implementation uses feed-rs + Podcasting 2.0 extension walker.
// Reference: docs/design/podcast/podcast-feeds.md §A.

use serde::{Deserialize, Serialize};
use url::Url;

/// Minimal parsed representation of a feed entry.
/// Full shape added in Step 1D.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ParsedPodcast {
    pub title: String,
    pub author: String,
    pub feed_url: Url,
    pub artwork_url: Option<Url>,
    pub episodes: Vec<ParsedEpisode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ParsedEpisode {
    pub guid: String,
    pub title: String,
    pub audio_url: Url,
    pub duration_s: f64,
    pub published_at_ms: u64,
    pub description: Option<String>,
}

/// Parse RSS/Atom/JSON Feed bytes into a `ParsedPodcast`.
/// Stub returns `Err`; real implementation uses `feed-rs`.
pub fn parse_feed(_bytes: &[u8], _feed_url: &Url) -> Result<ParsedPodcast, FeedError> {
    Err(FeedError::NotImplemented)
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum FeedError {
    #[error("feed parsing not yet implemented")]
    NotImplemented,
    #[error("invalid feed: {0}")]
    Invalid(String),
    #[error("network error: {0}")]
    Network(String),
}
