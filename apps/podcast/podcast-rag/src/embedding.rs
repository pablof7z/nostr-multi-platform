// EmbeddingCapability stub.
// Bridges to on-device embedding model (Apple NLEmbedding / SentenceTransformer).
// Reference: docs/design/podcast/podcast-rag.md §B.

use serde::{Deserialize, Serialize};

/// 384-dimension embedding vector (matches sqlite-vec schema).
pub type Embedding = Vec<f32>;

/// Request sent to the EmbeddingCapability.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EmbedRequest {
    pub text: String,
}

/// Response from the EmbeddingCapability.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EmbedResponse {
    pub embedding: Embedding,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("embedding capability not yet implemented")]
    NotImplemented,
    #[error("model error: {0}")]
    Model(String),
}
