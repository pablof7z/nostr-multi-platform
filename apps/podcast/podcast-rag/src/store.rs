// sqlite-vec vector store stub.
// Schema mirrors VectorDatabase.swift exactly (chunk_vectors, guest_vectors,
// insight_vectors — see docs/design/podcast/podcast-rag.md §A.2).

use serde::{Deserialize, Serialize};

use crate::embedding::Embedding;

/// Top-K retrieval result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RetrievedChunk {
    pub chunk_id: String,
    pub episode_id: String,
    pub podcast_id: String,
    pub start_s: f64,
    pub end_s: f64,
    pub distance: f32,
}

/// Vector store stub — real implementation uses sqlite-vec extension via rusqlite.
pub struct VectorStore;

impl VectorStore {
    /// Index a chunk embedding.
    pub fn index_chunk(
        &self,
        _chunk_id: &str,
        _episode_id: &str,
        _podcast_id: &str,
        _start_s: f64,
        _end_s: f64,
        _embedding: &Embedding,
    ) -> Result<(), StoreError> {
        Err(StoreError::NotImplemented)
    }

    /// Retrieve top-K chunks by cosine similarity.
    pub fn retrieve_chunks(
        &self,
        _query_embedding: &Embedding,
        _k: u32,
        _episode_id_filter: Option<&str>,
    ) -> Result<Vec<RetrievedChunk>, StoreError> {
        Err(StoreError::NotImplemented)
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum StoreError {
    #[error("vector store not yet implemented")]
    NotImplemented,
    #[error("sqlite error: {0}")]
    Sqlite(String),
}
