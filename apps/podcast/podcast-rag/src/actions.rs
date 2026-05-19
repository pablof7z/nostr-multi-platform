// RAG action stubs: Index, Retrieve, GetRecommendations.
// Reference: docs/design/podcast/podcast-rag.md §C.

use serde::{Deserialize, Serialize};

use podcast_core::domain::ids::{EmbeddingId, EpisodeId};

use crate::store::RetrievedChunk;

/// Index a transcript chunk (embed + store).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct IndexChunk {
    pub episode_id: EpisodeId,
    pub chunk_id: String,
    pub text: String,
    pub start_s: f64,
    pub end_s: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum IndexChunkOutput {
    Indexed { embedding_id: EmbeddingId },
}

/// Retrieve top-K chunks matching a natural-language query.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RetrieveChunks {
    pub query: String,
    pub k: u32,
    pub episode_id_filter: Option<EpisodeId>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum RetrieveChunksOutput {
    Results(Vec<RetrievedChunk>),
}

/// Get personalised podcast recommendations.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GetRecommendations {
    pub subscribed_podcast_ids: Vec<String>,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PodcastRecommendation {
    pub podcast_index_id: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum GetRecommendationsOutput {
    Recommendations(Vec<PodcastRecommendation>),
}
