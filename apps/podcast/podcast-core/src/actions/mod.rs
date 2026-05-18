// Action stubs for podcast-core.
// Full step machines in docs/design/podcast/podcast-core.md §D.

pub mod modules;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::domain::ids::*;
use crate::domain::records::SummaryStyle;

// --- Library lifecycle ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SubscribePodcast {
    pub feed_url: Url,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum SubscribePodcastOutput {
    Subscribed { podcast_id: PodcastId },
    AlreadySubscribed { podcast_id: PodcastId },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct UnsubscribePodcast {
    pub podcast_id: PodcastId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RefreshFeed {
    pub podcast_id: PodcastId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RefreshAllFeeds {}

// --- Download / processing chain ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DownloadEpisode {
    pub episode_id: EpisodeId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CancelDownload {
    pub episode_id: EpisodeId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DeleteDownload {
    pub episode_id: EpisodeId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EnqueueTranscription {
    pub episode_id: EpisodeId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SummarizeEpisode {
    pub episode_id: EpisodeId,
    pub style: SummaryStyle,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExtractChapters {
    pub episode_id: EpisodeId,
}

// --- Player ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Play {
    pub episode_id: EpisodeId,
    pub from_s: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum PlayOutput {
    Started,
    Resumed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Pause {}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Resume {}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Seek {
    pub to_s: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SkipForward {
    pub seconds: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SkipBack {
    pub seconds: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SetRate {
    pub rate: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Stop {}

// --- Insights ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StartInsightRecording {}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StopInsightRecording {
    pub episode_id: EpisodeId,
    pub capture_time_s: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum StopInsightRecordingOutput {
    InsightCreated { id: InsightId },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DeleteInsight {
    pub insight_id: InsightId,
}
