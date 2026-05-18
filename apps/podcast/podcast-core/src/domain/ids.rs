// Type-safe ID aliases for all podcast domain records.
// Using ulid::Ulid to match the nmp-core convention.

pub type PodcastId = ulid::Ulid;
pub type EpisodeId = ulid::Ulid;
pub type TranscriptId = ulid::Ulid;
pub type ChunkId = ulid::Ulid;
pub type ChapterId = ulid::Ulid;
pub type GuestId = ulid::Ulid;
pub type GuestContentId = ulid::Ulid;
pub type InsightId = ulid::Ulid;
pub type QueueEntryId = ulid::Ulid;
// EmbeddingId is owned here (not in podcast-rag) to avoid import cycles.
// podcast-rag depends on podcast-core (one-way); podcast-core must not
// depend on podcast-rag.
pub type EmbeddingId = ulid::Ulid;
