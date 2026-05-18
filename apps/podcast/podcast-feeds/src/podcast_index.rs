// Podcast Index API client stub (HMAC-SHA1 auth).
// Reference Swift: PodcastIndexService.swift (186 LOC).
// Full implementation in Step 1D.

use serde::{Deserialize, Serialize};
use url::Url;

/// Minimal representation of a Podcast Index search result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PodcastIndexPodcast {
    pub id: u64,
    pub title: String,
    pub author: Option<String>,
    pub artwork_url: Option<Url>,
    pub feed_url: Url,
    pub category_ids: Vec<u32>,
}

/// Search query stubs — wired to HttpCapability in Step 1D.
pub struct PodcastIndexClient;

impl PodcastIndexClient {
    /// Search for podcasts by term.
    pub async fn search(&self, _term: &str, _limit: u32) -> Result<Vec<PodcastIndexPodcast>, IndexError> {
        Err(IndexError::NotImplemented)
    }

    /// Return trending podcasts.
    pub async fn trending(&self, _limit: u32) -> Result<Vec<PodcastIndexPodcast>, IndexError> {
        Err(IndexError::NotImplemented)
    }

    /// Return podcasts by category name.
    pub async fn podcasts_by_category(
        &self,
        _category: &str,
        _limit: u32,
    ) -> Result<Vec<PodcastIndexPodcast>, IndexError> {
        Err(IndexError::NotImplemented)
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum IndexError {
    #[error("podcast index client not yet implemented")]
    NotImplemented,
    #[error("request failed: {0}")]
    Request(String),
    #[error("auth error")]
    Auth,
}
