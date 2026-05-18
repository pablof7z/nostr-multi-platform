// View module payload stubs for podcast-core.
// Full reactivity contracts in docs/design/podcast/podcast-core.md §C.

pub mod library;

use serde::{Deserialize, Serialize};

use crate::domain::records::SettingsRecord;

// --- Shared payload types ---

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PodcastRowPayload {
    pub id: String,
    pub title: String,
    pub author: String,
    pub artwork_url: Option<String>,
    pub episode_count: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct EpisodeRowPayload {
    pub id: String,
    pub title: String,
    pub podcast_title: String,
    pub podcast_artwork_url: Option<String>,
    pub summary: Option<String>,
    pub duration_str: String,
    pub download_state: String,
    pub active_job_kind: Option<String>,
    pub has_insights: bool,
    pub insights_count: u32,
    pub is_playing: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct InsightCardPayload {
    pub id: String,
    pub thought_text: String,
    pub excerpt_text: String,
    pub excerpt_start_s: f64,
    pub excerpt_end_s: f64,
    pub episode_title: String,
}

// --- View payloads (one per ViewModule) ---

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct LibraryView {
    pub podcasts: Vec<PodcastRowPayload>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct FeedView {
    pub episodes: Vec<EpisodeRowPayload>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct NowPlayingView {
    pub episode_id: Option<String>,
    pub podcast_id: Option<String>,
    pub title: String,
    pub podcast_title: String,
    pub artwork_url: Option<String>,
    pub progress_pct: f64,
    pub current_s: f64,
    pub duration_s: f64,
    pub state: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct InsightsView {
    pub cards: Vec<InsightCardPayload>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct SettingsView {
    pub settings: SettingsRecord,
    pub version: String,
    pub build: String,
}
