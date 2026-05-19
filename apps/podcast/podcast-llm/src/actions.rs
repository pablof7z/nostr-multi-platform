// LLM action stubs.
// Reference: docs/design/podcast/podcast-llm.md §D.

use serde::{Deserialize, Serialize};

use podcast_core::domain::ids::{EpisodeId, GuestId};
use podcast_core::domain::records::SummaryStyle;

// --- Summarize ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SummarizeEpisode {
    pub episode_id: EpisodeId,
    pub style: SummaryStyle,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum SummarizeEpisodeOutput {
    Summarized { summary: String },
}

// --- Extract chapters ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExtractChapters {
    pub episode_id: EpisodeId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExtractedChapter {
    pub title: String,
    pub summary: String,
    pub start_s: f64,
    pub end_s: f64,
    pub is_ad: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ExtractChaptersOutput {
    Extracted { chapters: Vec<ExtractedChapter> },
}

// --- Ask (RAG-grounded chat) ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AskQuestion {
    pub session_id: String,
    pub query: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChatTurn {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum AskQuestionOutput {
    Answered { turn: ChatTurn },
}

// --- Guest enrichment ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EnrichGuest {
    pub guest_id: GuestId,
    pub episode_id: EpisodeId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum EnrichGuestOutput {
    Enriched,
}

// --- Find relevant timestamp ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FindRelevantTimestamp {
    pub episode_id: EpisodeId,
    pub query: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum FindRelevantTimestampOutput {
    Found { timestamp_s: f64, context: String },
    NotFound,
}

// --- Match excerpt (insight capture) ---

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MatchExcerpt {
    pub episode_id: EpisodeId,
    pub thought_text: String,
    pub capture_time_s: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum MatchExcerptOutput {
    Matched {
        excerpt_text: String,
        start_s: f64,
        end_s: f64,
    },
    NoMatch,
}
