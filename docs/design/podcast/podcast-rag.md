# Step 1C — `apps/podcast/podcast-rag`

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md).
> Reference Swift sources: `Services/RAGService.swift` (257 LOC), `Services/VectorDatabase.swift` (259 LOC), `Services/RecommendationService.swift` (235 LOC).
> Capabilities consumed: `EmbeddingCapability`.

`podcast-rag` owns the embedding store and the retrieval functions. It is the bridge between domain records (transcripts, insights, guest content) and the LLM crate: every Ask / Insight / Recommendation flow asks `podcast-rag` for top-K matches, then hands the matches plus the query to `podcast-llm` for grounded generation.

---

## A. Vector-store choice: `sqlite-vec` (recommended)

### A.1 Comparison

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **`sqlite-vec`** (the `sqlite-vec` Rust crate, used by `rusqlite`/`sqlx` extensions) | Reference Swift app already uses it (`VectorDatabase.swift` imports `SQLiteVec` and calls `vec0`). Identical schema; identical semantics. Single SQLite file. Statically linkable. Tiny binary footprint. Sub-millisecond exact k-NN at 384-dim. No service, no separate process. | C extension; iOS static-linking is non-default and is the top risk (podcast-rmp flagged it as a spike — see [`lessons.md`](lessons.md)). | **Chosen.** |
| `sqlite-vss` (FAISS-backed) | Battle-tested for FAISS ops. | Heavier (~25 MB native lib), C++ FAISS deps, less actively maintained in 2026. iOS static-link harder than sqlite-vec. Different schema from the reference. | Reject. |
| `qdrant-client` (Rust) | First-class Rust client; horizontal scale; rich filters. | Requires a running Qdrant server — wrong shape for an offline-first mobile app. Out of scope. | Reject. |
| `lancedb` (Rust embedded) | Embedded; columnar; good at large scale. | Arrow/columnar overhead is overkill at our cardinality (≤ 5k chunks/episode × 100 episodes ≈ 500k vectors). Schema diverges from reference. | Reject. |
| Pure-Rust HNSW (`hnsw_rs`, `instant-distance`) | No C extension; pure Rust; no FFI surface to worry about. | We give up on-disk persistence (memory-only) or roll our own (significant complexity). Recall-vs-build-time trade tuning becomes ours. | Considered as the fallback if sqlite-vec iOS spike fails. |

### A.2 Schema (mirrors `VectorDatabase.swift` exactly)

```sql
-- chunk_vectors and chunk_metadata
CREATE VIRTUAL TABLE chunk_vectors USING vec0(
    chunk_id TEXT PRIMARY KEY,
    embedding float[384]
);
CREATE TABLE chunk_metadata (
    chunk_id TEXT PRIMARY KEY,
    episode_id TEXT,
    podcast_id TEXT,
    start_time REAL,
    end_time REAL
);
CREATE INDEX chunk_metadata_episode_idx ON chunk_metadata(episode_id);
CREATE INDEX chunk_metadata_podcast_idx ON chunk_metadata(podcast_id);

-- guest_vectors and guest_metadata
CREATE VIRTUAL TABLE guest_vectors USING vec0(
    content_id TEXT PRIMARY KEY,
    embedding float[384]
);
CREATE TABLE guest_metadata (
    content_id TEXT PRIMARY KEY,
    guest_id TEXT,
    source TEXT
);

-- insight_vectors and insight_metadata
CREATE VIRTUAL TABLE insight_vectors USING vec0(
    insight_id TEXT PRIMARY KEY,
    embedding float[384]
);
CREATE TABLE insight_metadata (
    insight_id TEXT PRIMARY KEY,
    episode_id TEXT,
    podcast_id TEXT
);
```

384 dimensions matches the reference (`AIService.swift::generateSimpleEmbedding` produces 384-dim). Real embeddings switch the dim constant only via a migration ADR (the choice locks against future model swaps).

### A.3 Storage location

Per-app SQLite DB file at `<data_dir>/podcast/vectors.sqlite`. Single file per device, not per-account — the M11 reference Swift app has no multi-account model. Multi-account is post-M11 (M8 in the NMP plan); the schema is forward-compatible (one row per `(account_id, chunk_id)` becomes the composite key in a v2 migration).

---

## B. Embedding model

### B.1 The reference reality

`AIService.swift::generateSimpleEmbedding` is a **hash-based fake** (positional sum of `word.hashValue % 384`, then L2-normalized). The Swift app's RAG is functionally non-functional today — retrieval scores are dominated by surface keyword overlap and noise. This is documented honestly here; the NMP rebuild is where the RAG actually starts working.

### B.2 Choice

`EmbeddingCapability` (see [`capabilities.md`](capabilities.md)) is the typed bridge. On iOS the impl is **CoreML running `BAAI/bge-small-en-v1.5`** (384 dims, ~33 MB CoreML package, ~12 ms per chunk on iPhone 12). Convert with `coremltools` from the HF model at build time, ship the `.mlpackage` in the app bundle.

On Android/Desktop (post-M11): `fastembed-rs` with the same model — pure-Rust, ORT-backed, identical numerical output to within float-rounding.

Why `bge-small-en-v1.5` and not OpenAI `text-embedding-3-small` or `nomic-embed-text`:

- **Free / offline / private** — matches the Swift app's no-API-key onboarding.
- **384 dims** — fits the existing sqlite-vec schema; smallest competitive English model.
- **Apache-2 license** — no usage restrictions.
- **Latency** — on-device on iPhone 12, ~12 ms per 256-token chunk.

If a future user wants a higher-quality model (e.g., a 1024-dim `bge-large-en-v1.5`), it's an ADR + a migration.

---

## C. Chunking

Migrated from `TranscriptionService.swift::createChunksFromText` and `ProcessingQueue.swift::processTranscription`. The Swift code targets ~30 seconds per chunk based on character-time-rate estimation. Rust impl in `podcast-rag/src/chunking.rs`:

```rust
pub fn chunk_transcript(full_text: &str, total_duration_s: f64, real_chunks: &[(String, f64, f64)]) -> Vec<TranscriptChunkRecord> {
    if !real_chunks.is_empty() {
        return real_chunks.iter().enumerate()
            .map(|(i, (text, start, end))| TranscriptChunkRecord {
                id: ChunkId::new(),
                transcript_id: TranscriptId::nil(),   // set by caller
                text: text.clone(),
                start_s: *start,
                end_s: *end,
                chunk_index: i as u32,
                embedding_id: None,
            })
            .collect();
    }
    // Sentence-split fallback (same logic as Swift impl):
    estimate_chunks_from_sentences(full_text, total_duration_s)
}
```

Target chunk length: 30 s ± 10 s; max 1 paragraph (3-5 sentences). This matches the empirical sweet spot in `RecommendationService.swift` and the reference UX where transcript-tap-to-play resolves at "thought-sized" granularity.

---

## D. Action modules

### D.1 `IndexChunk`

```rust
pub struct IndexChunk { pub chunk_id: ChunkId }
pub enum IndexChunkOutput { Indexed { embedding_id: EmbeddingId } }
```

Step: load chunk text → `AwaitCapability(EmbeddingCapability::Embed { text })` → write to `chunk_vectors` + `chunk_metadata`. On success: backfill `TranscriptChunkRecord.embedding_id`.

### D.2 `IndexInsight`, `IndexGuestContent`

Same shape, different table. Insight embedding combines `thought_text + " | Context: " + excerpt_text` (mirrors `InsightService.swift::processInsight`).

### D.3 `Retrieve`

```rust
pub struct Retrieve {
    pub query: String,
    pub limit: u32,
    pub filter: Option<RagFilter>,
    pub include_insights: bool,
}

pub enum RagFilter {
    ByEpisodes(Vec<EpisodeId>),
    ByPodcasts(Vec<PodcastId>),
    ByGuest(GuestId),
}

pub enum RetrieveOutput { Results { results: Vec<RagResultPayload> } }

pub struct RagResultPayload {
    pub kind: RagResultKind,    // Chunk | Insight | GuestContent
    pub source_id: Ulid,
    pub text: String,
    pub score: f32,
    pub episode_title: Option<String>,
    pub podcast_title: Option<String>,
    pub timestamp_s: Option<f64>,
    pub is_insight: bool,
}
```

Step machine:

1. `EmbedQuery` → `AwaitCapability(EmbeddingCapability::Embed)`.
2. `Search` — synchronous SQL against sqlite-vec; `ORDER BY distance LIMIT k`. Both chunk and insight tables queried in parallel when `include_insights=true`.
3. `Hydrate` — join results with `EpisodeRecord`/`PodcastRecord` to populate titles for display.
4. `Boost insights by 1.2x` (mirrors `RAGService.swift::searchWithContext`).
5. Sort by score desc, take top `limit`.

Time budget: ≤ 50 ms p99 on iPhone 12 at 500k vectors. Validated by the M11 perf gate.

### D.4 `GetRecommendations`, `GetPersonalizedHero`, `GetUserTopics`, `SearchEpisodes`

Migrated from `RecommendationService.swift`. Each follows the same Embed → Search → Hydrate → format pattern. `GetRecommendations` falls back to `PodcastFeeds::Trending` when the user's taste profile is empty (no listened episodes).

### D.5 `DeleteEpisodeVectors`, `DeleteGuestVectors`, `DeleteInsightVector`, `ClearAllVectors`

Same shape as `VectorDatabase.swift` cascade methods. Called by domain-cascade logic in `podcast-core` when a parent record is deleted.

---

## E. Storage backend

```rust
pub trait VectorStore: Send + Sync {
    fn insert_chunk(&self, chunk_id: &str, embedding: &[f32], meta: ChunkMeta) -> Result<()>;
    fn search_chunks(&self, query: &[f32], k: usize) -> Result<Vec<ChunkHit>>;
    // ...one method per Swift VectorDatabase fn
}

pub struct SqliteVecStore { conn: rusqlite::Connection /* with vec0 loaded */ }
impl VectorStore for SqliteVecStore { /* ... */ }
```

Why a trait: makes the in-memory test backend trivial (a `Vec<(String, Vec<f32>, ChunkMeta)>` with brute-force k-NN). The `MockVectorStore` is what `nmp-testing` runs against.

---

## F. Recommendation pipeline notes

The Swift `RecommendationService` has two algorithm halves:

1. **Taste profile** — average embedding of last 20 listened episodes' (title + summary). The Rust port keeps this but uses real embeddings.
2. **Trending fallback** — when `tasteProfile.isEmpty`, return Podcast Index trending. Migrated as-is to `podcast-feeds::Trending`.

The "Episodes about \(topic)" reason string is a heuristic — `extractTopic()` keyword-matches against a fixed topic list. Kept verbatim in Rust; the topic list is `const TOPIC_KEYWORDS: &[&str]`. Future improvement is an ADR.

---

## G. Tests

- Schema round-trip: insert 1k vectors → search → assert all retrievable.
- Recall@10 on a small synthetic corpus (5 episodes, 100 chunks, hand-labeled queries): ≥ 0.7 (sanity).
- Cascade test: deleting an episode triggers vector-deletion in `chunk_vectors` + `chunk_metadata`.
- Latency test: 100k vectors, p99 search latency < 50 ms on developer machine.
- Capability-failure test: `EmbeddingCapability` returns error → `Retrieve` reduces to `Fail { reason }` with `toast` set; no partial writes.

---

## H. `Cargo.toml`

```toml
[package]
name = "podcast-rag"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
nmp-core = { path = "../../../crates/nmp-core" }
podcast-core = { path = "../podcast-core" }
sqlite-vec = "0.1"            # Rust bindings to the vec0 SQLite extension
rusqlite = { version = "0.31", features = ["bundled", "load_extension"] }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
ulid = "1"
async-trait = "0.1"
futures = "0.3"
tracing = "0.1"
thiserror = "1"

[dev-dependencies]
nmp-testing = { path = "../../../crates/nmp-testing" }
```

**iOS bundling note.** `rusqlite` with `bundled` builds `libsqlite3` from source statically. Loading the `sqlite-vec` extension on iOS requires either (a) statically linking the `vec0` symbols (preferred) via a `build.rs` patch, or (b) shipping the dylib in the app bundle and using `load_extension`. The path-(a) approach is the goal; the spike to confirm landed in [`risks.md`](risks.md).
