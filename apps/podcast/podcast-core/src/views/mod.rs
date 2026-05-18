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
    /// The RSS/Atom feed URL for this podcast — exposed so the host platform
    /// can re-fetch bytes for pull-to-refresh without maintaining a separate
    /// URL index on the Kotlin/Swift side.
    pub feed_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct EpisodeRowPayload {
    pub id: String,
    pub title: String,
    pub podcast_title: String,
    pub podcast_artwork_url: Option<String>,
    /// The ULID string of the parent podcast. Projected so the mini-player can
    /// navigate to EpisodeDetail from any screen without tracking a separate
    /// podcast-id in the host UI (D5 — zero Kotlin business logic).
    pub podcast_id: String,
    pub summary: Option<String>,
    pub duration_str: String,
    /// Human-readable publication date, e.g. "Jan 1, 2024". Empty string when
    /// `published_at_ms` is 0 (feed omitted the date).
    pub pub_date_str: String,
    pub download_state: String,
    pub active_job_kind: Option<String>,
    pub has_insights: bool,
    pub insights_count: u32,
    pub is_playing: bool,
    /// The streaming audio URL for this episode (from the RSS enclosure element).
    /// Empty string when the feed omitted the enclosure — UI must gate playback
    /// on this being non-empty (D6 honest state).
    pub audio_url: String,
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
