// D0: RAG/vector-store nouns live here, never in nmp-core.
// Owns: EmbeddingCapability, sqlite-vec store, Index/Retrieve/Chat actions.
// Reference Swift: RAGService.swift (257 LOC), VectorDatabase.swift (259 LOC),
//                  RecommendationService.swift (235 LOC).
// Full implementation target: docs/design/podcast/podcast-rag.md.

pub mod actions;
pub mod embedding;
pub mod store;

#[cfg(test)]
mod tests {
    use super::embedding::EmbedRequest;
    use super::store::{StoreError, VectorStore};

    #[test]
    fn podcast_rag_vector_store_stub_returns_not_implemented() {
        let store = VectorStore;
        let embedding = vec![0.0f32; 384];
        let result = store.retrieve_chunks(&embedding, 5, None);
        assert!(matches!(result, Err(StoreError::NotImplemented)));
    }

    #[test]
    fn podcast_rag_embed_request_serializes() {
        let req = EmbedRequest {
            text: "hello world".to_string(),
        };
        let json = serde_json::to_string(&req).expect("serialize embed request");
        assert!(json.contains("hello world"));
    }
}
