// Domain record stubs for podcast-core.
// Bodies are minimal but structurally complete so the workspace compiles.
// Full implementations land in Step 1A per docs/design/podcast/podcast-core.md §B.

use serde::{Deserialize, Serialize};
use url::Url;

use super::ids::*;

// --- Podcast ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PodcastRecord {
    pub id: PodcastId,
    pub feed_url: Url,
    pub title: String,
    pub author: String,
    pub artwork_url: Option<Url>,
    pub subscribed_at_ms: u64,
    pub last_refreshed_ms: Option<u64>,
}

// --- Episode ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum DownloadState {
    NotDownloaded,
    Downloading { ledger_action_id: String },
    Downloaded,
    Failed { reason: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EpisodeRecord {
    pub id: EpisodeId,
    pub podcast_id: PodcastId,
    pub guid: String,
    pub title: String,
    pub ai_summary: Option<String>,
    pub description_text: Option<String>,
    pub audio_url: Url,
    pub duration_s: f64,
    pub published_at_ms: u64,
    pub download_state: DownloadState,
    pub local_audio_path: Option<String>,
    pub playback_position_s: f64,
    pub has_been_played: bool,
    pub transcript_id: Option<TranscriptId>,
    pub insight_ids: Vec<InsightId>,
    pub guest_ids: Vec<GuestId>,
}

// --- Transcript ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TranscriptRecord {
    pub id: TranscriptId,
    pub episode_id: EpisodeId,
    pub full_text: String,
    pub language: String,
    pub generated_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TranscriptChunkRecord {
    pub id: ChunkId,
    pub transcript_id: TranscriptId,
    pub text: String,
    pub start_s: f64,
    pub end_s: f64,
    pub chunk_index: u32,
    pub embedding_id: Option<EmbeddingId>,
}

// --- Chapter ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChapterRecord {
    pub id: ChapterId,
    pub transcript_id: TranscriptId,
    pub title: String,
    pub summary: String,
    pub start_s: f64,
    pub end_s: f64,
    pub chapter_index: u32,
    pub is_ad: bool,
}

// --- Guest ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum GuestContentSource {
    Twitter,
    Blog,
    OtherPodcast,
    Wikipedia,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GuestRecord {
    pub id: GuestId,
    pub name: String,
    pub normalized_name: String,
    pub bio: Option<String>,
    pub twitter_handle: Option<String>,
    pub website_url: Option<Url>,
    pub episode_ids: Vec<EpisodeId>,
    pub guest_content_ids: Vec<GuestContentId>,
    pub last_enriched_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GuestContentRecord {
    pub id: GuestContentId,
    pub guest_id: GuestId,
    pub source: GuestContentSource,
    pub text: String,
    pub url: Option<Url>,
    pub published_at_ms: Option<u64>,
    pub embedding_id: Option<EmbeddingId>,
}

// --- Insight ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct InsightRecord {
    pub id: InsightId,
    pub episode_id: EpisodeId,
    pub created_at_ms: u64,
    pub thought_text: String,
    pub thought_audio_path: Option<String>,
    pub excerpt_text: String,
    pub excerpt_start_s: f64,
    pub excerpt_end_s: f64,
    pub embedding_id: Option<EmbeddingId>,
}

// --- Settings ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum SummaryStyle {
    Brief,
    Detailed,
    Bullets,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SettingsRecord {
    pub skip_forward_s: f64,
    pub skip_backward_s: f64,
    pub default_rate: f32,
    pub allow_cellular: bool,
    pub auto_transcribe: bool,
    pub auto_summarize: bool,
    pub auto_extract_chapters: bool,
    pub default_summary_style: SummaryStyle,
    pub skip_ads: bool,
}

impl Default for SettingsRecord {
    fn default() -> Self {
        Self {
            skip_forward_s: 30.0,
            skip_backward_s: 15.0,
            default_rate: 1.0,
            allow_cellular: false,
            auto_transcribe: false,
            auto_summarize: false,
            auto_extract_chapters: false,
            default_summary_style: SummaryStyle::Brief,
            skip_ads: false,
        }
    }
}
