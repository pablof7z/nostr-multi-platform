# Step 1A — `apps/podcast/podcast-core`

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md)
> Sibling crates: [`podcast-llm.md`](podcast-llm.md), [`podcast-rag.md`](podcast-rag.md), [`podcast-feeds.md`](podcast-feeds.md), [`capabilities.md`](capabilities.md).
> Substrate reference: [`../kernel-substrate.md`](../kernel-substrate.md).

`podcast-core` is the **central app crate**. It owns every domain record (Podcast/Episode/Transcript/Chapter/Guest/Insight/PlayerState/QueueEntry/Activity/Subscription/SettingsRecord) and every non-LLM, non-RAG, non-feed-parsing view + action module. It depends on capabilities defined in [`capabilities.md`](capabilities.md) but does not implement them.

---

## A. `Cargo.toml`

```toml
[package]
name = "podcast-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
nmp-core = { path = "../../../crates/nmp-core" }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
ulid = "1"               # action / record IDs
url = "2"                # URL type for feed/audio/artwork URLs
async-trait = "0.1"      # for ActionModule reduce-step traits
tracing = "0.1"          # structured logging (no OSLog/println)
futures = "0.3"          # for BoxFuture in ActionModule
thiserror = "1"

[dev-dependencies]
nmp-testing = { path = "../../../crates/nmp-testing" }
```

No tokio. The actor's runtime is owned by `nmp-core`; action modules return state machines, not async tasks (see ADR-0009).

---

## B. DomainModules

Eight `DomainModule`s, one per top-level record type. Each lives in `podcast-core/src/domain/<noun>.rs`.

### B.1 `podcasts::PodcastsModule` (NAMESPACE = `podcast.podcasts`, SCHEMA_VERSION = 1)

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PodcastRecord {
    pub id: PodcastId,         // ulid-typed
    pub feed_url: Url,
    pub title: String,
    pub author: String,
    pub artwork_url: Option<Url>,
    pub subscribed_at_ms: u64,
    pub last_refreshed_ms: Option<u64>,
}

impl DomainModule for PodcastsModule {
    const NAMESPACE: &'static str = "podcast.podcasts";
    const SCHEMA_VERSION: u32 = 1;

    fn migrations() -> Vec<DomainMigration> { vec![] }

    fn indexes() -> Vec<DomainIndex> {
        vec![DomainIndex {
            name: "by_feed_url",
            key_fn: |bytes| serde_json::from_slice::<PodcastRecord>(bytes)
                .ok()
                .map(|p| p.feed_url.to_string().into_bytes()),
        }]
    }

    fn register(reg: &mut DomainRegistry) {
        reg.register_record::<PodcastRecord>();
    }
}
```

### B.2 `episodes::EpisodesModule` (NAMESPACE = `podcast.episodes`)

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum DownloadState {
    NotDownloaded,
    Downloading { ledger_action_id: ActionId },
    Downloaded,
    Failed { reason: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EpisodeRecord {
    pub id: EpisodeId,
    pub podcast_id: PodcastId,
    pub guid: String,           // stable per-feed identifier (parsed from RSS)
    pub title: String,
    pub ai_summary: Option<String>,
    pub description_text: Option<String>,
    pub audio_url: Url,
    pub duration_s: f64,
    pub published_at_ms: u64,
    pub download_state: DownloadState,
    pub local_audio_path: Option<String>,  // platform-relative; capability resolves
    pub playback_position_s: f64,
    pub has_been_played: bool,
    pub transcript_id: Option<TranscriptId>,
    pub insight_ids: Vec<InsightId>,
    pub guest_ids: Vec<GuestId>,
}
```

Indexes: `by_podcast_id`, `by_published_at_ms` (desc), `by_guid_within_podcast` (composite). Cascade semantics: deleting a `PodcastRecord` enqueues `Action::DeleteEpisode { id }` per affected episode (Rust-side, not SQL).

### B.3 `transcripts::TranscriptsModule` (NAMESPACE = `podcast.transcripts`)

```rust
pub struct TranscriptRecord {
    pub id: TranscriptId,
    pub episode_id: EpisodeId,
    pub full_text: String,
    pub language: String,            // BCP-47
    pub generated_at_ms: u64,
}

pub struct TranscriptChunkRecord {
    pub id: ChunkId,
    pub transcript_id: TranscriptId,
    pub text: String,
    pub start_s: f64,
    pub end_s: f64,
    pub chunk_index: u32,
    pub embedding_id: Option<EmbeddingId>,   // pointer into podcast-rag vector store
}
```

Indexes: `by_episode_id`, `chunks_by_transcript_id_then_chunk_index`.

### B.4 `chapters::ChaptersModule` (NAMESPACE = `podcast.chapters`)

```rust
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
```

Indexes: `by_transcript_id_then_chapter_index`.

### B.5 `guests::GuestsModule` (NAMESPACE = `podcast.guests`)

```rust
pub struct GuestRecord {
    pub id: GuestId,
    pub name: String,
    pub normalized_name: String,     // lowercase, trimmed
    pub bio: Option<String>,
    pub twitter_handle: Option<String>,
    pub website_url: Option<Url>,
    pub episode_ids: Vec<EpisodeId>,
    pub guest_content_ids: Vec<GuestContentId>,
    pub last_enriched_at_ms: Option<u64>,
}

pub struct GuestContentRecord {
    pub id: GuestContentId,
    pub guest_id: GuestId,
    pub source: GuestContentSource,        // enum Twitter|Blog|OtherPodcast|Wikipedia
    pub text: String,
    pub url: Option<Url>,
    pub published_at_ms: Option<u64>,
    pub embedding_id: Option<EmbeddingId>,
}
```

### B.6 `insights::InsightsModule` (NAMESPACE = `podcast.insights`)

Mirrors `Models/Insight.swift` exactly: `thought_text`, `thought_audio_path`, `excerpt_text`, `excerpt_start_s`, `excerpt_end_s`, `embedding_id?`. Indexes: `by_episode_id`, `by_created_at_ms` (desc).

### B.7 `subscriptions::SubscriptionsModule`

Not strictly necessary if subscription state is implicit ("a `PodcastRecord` exists ⇒ user is subscribed"). We make it explicit anyway for the M11 exit gate's "subscribe to 10 real podcasts" check and to support future Nostr-discovered "known but unsubscribed" podcasts (cf. podcast-rmp parity plan §L2):

```rust
pub struct SubscriptionRecord {
    pub podcast_id: PodcastId,
    pub status: SubscriptionStatus,      // Subscribed | KnownNotSubscribed
    pub auto_download: AutoDownloadPolicy,
    pub last_refreshed_ms: Option<u64>,
}
```

### B.8 `settings::SettingsModule`

One singleton record keyed by the active `IdentityScope::AppLocal { namespace: "podcast.app", id: "default" }`. Mirrors `Models/Settings.swift` fields.

### B.9 `queue::QueueModule`

The user-curated playback queue (distinct from the `ProcessingQueue` action ledger):

```rust
pub struct QueueEntryRecord {
    pub id: QueueEntryId,
    pub episode_id: EpisodeId,
    pub position: u32,        // 0-indexed; reorder updates positions
    pub added_at_ms: u64,
}
```

Not visible in `../podcast` today (the Swift app's `QueueView` is the processing queue, not a playback queue). M11 ships it **disabled by default** behind a Settings flag — present for the architecture proof but invisible until enabled. UI deferred.

### B.10 `activity::ActivityProjection`

Not a separate `DomainModule` — it's a **projection cache** computed over `EpisodeRecord` (filters: downloaded / transcribed / inProgress / unplayed). Lives in `podcast-core/src/projections/activity.rs`. Used by `ActivityViewModule`.

### B.11 Type aliases

```rust
pub type PodcastId = ulid::Ulid;
pub type EpisodeId = ulid::Ulid;
pub type TranscriptId = ulid::Ulid;
pub type ChunkId = ulid::Ulid;
pub type ChapterId = ulid::Ulid;
pub type GuestId = ulid::Ulid;
pub type GuestContentId = ulid::Ulid;
pub type InsightId = ulid::Ulid;
pub type QueueEntryId = ulid::Ulid;
pub type EmbeddingId = ulid::Ulid;   // defined here in podcast-core; podcast-rag depends on podcast-core (one-way)
```

UUIDv7 would also work; ulid chosen to match existing `nmp-core` convention.

**`EmbeddingId` ownership (ADR note).** `EmbeddingId` is defined in `podcast-core`, not in `podcast-rag`. This breaks any potential import cycle: `podcast-rag` already depends on `podcast-core` (one-way); `podcast-core` must not depend on `podcast-rag`. `podcast-rag` action modules return `Indexed { embedding_id: EmbeddingId }` events; `podcast-core` consumes those events to backfill `TranscriptChunkRecord.embedding_id`, `InsightRecord.embedding_id`, and `GuestContentRecord.embedding_id`. The writer of all three fields is always `podcast-core`; `podcast-rag` only produces and returns `EmbeddingId` values — it never writes a `*Record`.

---

## C. ViewModules

17 `ViewModule`s. Each in `podcast-core/src/views/<name>.rs`. All share `View*` payload conventions: pre-formatted strings (per doctrine D1), no business logic in Swift, composite-keyed `ViewDependencies` (per ADR-0001).

| View module | Spec | Payload (top-level fields) | Dependencies |
|---|---|---|---|
| `LibraryViewModule` | `LibrarySpec {}` | `LibraryView { podcasts: Vec<PodcastRowPayload> }` | `podcast.podcasts/*` inserts/updates |
| `PodcastDetailViewModule` | `PodcastDetailSpec { podcast_id }` | `{ podcast: PodcastHeader, episodes_desc: Vec<EpisodeRowPayload> }` | `podcast.podcasts/{id}`, `podcast.episodes:by_podcast_id={id}` |
| `EpisodeRowViewModule` | `EpisodeRowSpec { episode_id }` | `EpisodeRowPayload { id, title, podcast_title, podcast_artwork_url?, summary?, duration_str, download_state, active_job_kind?, has_insights, insights_count, is_playing }` | `podcast.episodes/{id}`, `podcast.podcasts/{episode.podcast_id}`, derived from `ledger/active_jobs/{episode_id}`, derived from `now_playing` |
| `EpisodeDetailViewModule` | `EpisodeDetailSpec { episode_id }` | `{ header: EpisodeHeader, summary?, description?, insights: Vec<InsightCardPayload> }` | episode + insights + podcast |
| `FeedViewModule` | `FeedSpec { limit }` | `{ episodes: Vec<EpisodeRowPayload> }` (cross-podcast, desc by published_at_ms) | `podcast.episodes:all` window |
| `NowPlayingViewModule` | `NowPlayingSpec {}` | `NowPlayingView { episode_id, podcast_id?, title, podcast_title?, artwork_url?, progress_pct, current_s, duration_s, state }` | `audio_capability/state`, `audio_capability/tick` (high-frequency, coalesced ≤ 4 Hz per ADR-0002) |
| `PlayerSheetViewModule` | `PlayerSheetSpec {}` | `{ episode, summary?, chapters: ChaptersPayload, guests: Vec<GuestChip>, capture_state }` | now_playing + chapters + guests + capture_state |
| `MiniPlayerViewModule` | `MiniPlayerSpec {}` | same shape as NowPlaying minus full controls | now_playing |
| `ChaptersViewModule` | `ChaptersSpec { episode_id }` | `{ chapters: Vec<ChapterPayload>, current_index?, is_extracting, search_result? }` | chapters + ledger/active_jobs/{episode_id, kind=ExtractChapters} |
| `TranscriptViewModule` | `TranscriptSpec { episode_id }` | `{ chunks: Vec<ChunkPayload>, current_chunk?, is_transcribing, is_summarizing, summary? }` | transcript + chunks + ledger/active_jobs/{ep, kind ∈ {Transcribe, Summarize}} |
| `ProcessingQueueViewModule` | `ProcessingQueueSpec {}` | `{ active, completed, failed }` (each is `Vec<QueueJobPayload>`) | the kernel action ledger — selector: `namespace LIKE 'podcast.*'` |
| `ActivityViewModule` | `ActivitySpec { filter }` | `{ stats: ActivityStatsPayload, episodes: Vec<EpisodeStatusRow> }` | episodes (all) + projection cache |
| `DiscoverViewModule` | `DiscoverSpec { search_text? }` | split — see [`wiring.md`](wiring.md) §Discover (sub-views: Hero, Trending, ForYou, Categories, Topics, SearchResults) | each sub-spec has its own dependencies |
| `PodcastSheetViewModule` | `PodcastSheetSpec { podcast_index_id }` | `{ metadata: PodcastIndexPodcast, is_already_subscribed }` | `podcast_index_search_cache/{id}`, `podcasts/*` for subscribed-set |
| `InsightsViewModule` | `InsightsSpec { episode_id? }` | `{ cards: Vec<InsightCardPayload> }` | insights + episodes + podcasts (for header) |
| `AskViewModule` | `AskSpec { session_id }` | `{ messages: Vec<ChatTurnPayload>, suggested: Vec<String>, episode_count, hours_listened_str }` | chat session state (in `podcast-llm`) + episodes (counters) |
| `SettingsViewModule` | `SettingsSpec {}` | `{ settings: SettingsRecord, version, build }` | settings record |

Reactivity follows ADR-0001 (composite keys, broad-axis guardrails), ADR-0002 (≤60 Hz/view, audio-tick views capped at 4 Hz), ADR-0003 (hot-set working budget; only currently-open views materialize payloads).

---

## D. ActionModules

Roughly 24 `ActionModule`s. Each in `podcast-core/src/actions/<verb>.rs`. Output enums quoted below; full step machines in the implementing crate's source (this doc is the contract, not the implementation).

### D.1 Library lifecycle

```rust
pub struct SubscribePodcast { pub feed_url: Url }
pub enum SubscribePodcastOutput { Subscribed { podcast_id: PodcastId }, AlreadySubscribed { podcast_id: PodcastId } }
// step machine: validate URL → dispatch PodcastFeeds::FetchFeed → on success write PodcastRecord + EpisodeRecord*.
// atomic: feed-fetch is durable in ledger; podcast+episodes insert is one actor message.

pub struct UnsubscribePodcast { pub podcast_id: PodcastId }
pub enum UnsubscribePodcastOutput { Unsubscribed }

pub struct RefreshFeed { pub podcast_id: PodcastId }
pub enum RefreshFeedOutput { Refreshed { new_episode_ids: Vec<EpisodeId> } }

pub struct RefreshAllFeeds {}
pub enum RefreshAllFeedsOutput { Refreshed { feeds_refreshed: u32, new_episodes_total: u32 } }
```

### D.2 Download / processing chain

```rust
pub struct DownloadEpisode { pub episode_id: EpisodeId }
pub enum DownloadEpisodeOutput { Downloaded { local_path: String } }
// AwaitCapability: HttpCapability::Download { url, on_progress, on_complete }.
// On completion: write Episode.local_audio_path + flip Episode.download_state.
// If Settings.auto_transcribe: ctx.dispatch(EnqueueTranscription).

pub struct CancelDownload { pub episode_id: EpisodeId }
pub struct DeleteDownload { pub episode_id: EpisodeId }

pub struct EnqueueTranscription { pub episode_id: EpisodeId }
// AwaitCapability: TranscriptionCapability::Transcribe { local_path, language }.
// On completion: write Transcript + TranscriptChunk*. If Settings.auto_summarize: ctx.dispatch(SummarizeEpisode).

pub struct SummarizeEpisode { pub episode_id: EpisodeId, pub style: SummaryStyle }
// AwaitCapability: AppleIntelligenceCapability::Generate { prompt, style } OR PodcastLlm route.

pub struct ExtractChapters { pub episode_id: EpisodeId }
// AwaitCapability: AppleIntelligenceCapability::Generate ; parses out the CHAPTER|... format ;
// writes Chapter records.
```

### D.3 Player

```rust
pub struct Play { pub episode_id: EpisodeId, pub from_s: Option<f64> }
pub enum PlayOutput { Started, Resumed }
// AwaitCapability: AudioPlaybackCapability::Load { url_or_path, start_s }.
// Updates: NowPlaying view module gets a snapshot; clears lastSkippedAdChapter.

pub struct Pause {}
pub struct Resume {}
pub struct Seek { pub to_s: f64 }
pub struct SkipForward { pub seconds: f64 }
pub struct SkipBack { pub seconds: f64 }
pub struct SetRate { pub rate: f32 }
pub struct Stop {}
pub struct MarkPlayed { pub episode_id: EpisodeId }
pub struct UpdatePlaybackPosition { pub episode_id: EpisodeId, pub position_s: f64 }
// Position updates are batched: AudioPlaybackCapability emits Tick events; orchestrator persists every 5 s.
```

### D.4 Queue

```rust
pub struct EnqueueEpisode { pub episode_id: EpisodeId }
pub struct ReorderQueue { pub entries: Vec<QueueEntryId> }
pub struct ClearQueue {}
```

### D.5 Insights

```rust
pub struct StartInsightRecording {}
pub enum StartInsightRecordingOutput { Recording { recording_id: Ulid } }
// AwaitCapability: VoiceRecordingCapability::Start.

pub struct StopInsightRecording { pub recording_id: Ulid, pub episode_id: EpisodeId, pub capture_time_s: f64 }
pub enum StopInsightRecordingOutput { InsightCreated { id: InsightId } }
// step machine: stop capability → transcribe → call podcast-llm::MatchExcerpt → embed → insert.

pub struct DeleteInsight { pub insight_id: InsightId }
```

### D.6 Discovery

```rust
pub struct SearchPodcasts { pub query: String, pub limit: u32 }
// AwaitCapability: HttpCapability through PodcastFeeds::SearchExternal { query }; podcast-rag may rerank.

pub struct ImportRss { pub url: Url }
pub struct ImportOpml { pub opml_xml: String }
// ImportOpml is additive beyond strict ../podcast parity (see parent §1).
// Parses OPML, dispatches SubscribePodcast for each feed URL atomically.
```

### D.7 Settings

```rust
pub struct UpdateSettings { pub patch: SettingsPatch }
// SettingsPatch is field-update set (Option<T> per field).

pub struct ClearImageCache {}
// AwaitCapability: HttpCapability::ClearLocalCache or direct file deletion via FilePickerCapability cousin.
```

### D.8 Atomicity contract (per [`../kernel-substrate.md`](../kernel-substrate.md) §4)

Every action that mutates more than one record uses `ActionTransition::Complete` only after all writes are durable in the same actor message tick. Long-running capability-bound steps use `AwaitCapability`; the ledger row carries the in-flight state so restart recovery resumes from the last checkpoint, never silently divergent. Per RMP bible commandment #7 (idempotent capability lifecycle) every action is replayable from its current step.

---

## E. Projection caches (kernel-owned, module-defined)

Three projection caches power the view modules without duplicating event-store scans on every tick:

- `now_playing` — derived from `AudioPlaybackCapability::Tick` events. Single row. Coalesced.
- `active_jobs_by_episode` — derived from action ledger inserts/transitions. Map<EpisodeId, JobKind>.
- `episodes_by_published_desc` — sorted index, materialised over `EpisodeRecord` for the Feed/Activity views. Recomputed only on episode insert (incremental).

Per ADR-0001, projection cache changes feed `ProjectionChange` events into `ViewModule::on_projection_changed`. Each is registered as a `ProjectionCache` in `nmp-core::substrate::projections` (kernel-owned trait; modules opt in).

---

## F. Public surface for `nmp gen modules`

The codegen tool reads `apps/podcast/nmp.toml`:

```toml
[app]
name = "podcast"
display_name = "NMP Podcast"
bundle_id = "com.example.nmppodcast"

[modules]
kernel = "nmp-core"
protocol = []                # M11 has no Nostr protocol modules.
app = ["podcast-core", "podcast-llm", "podcast-rag", "podcast-feeds"]

[platforms]
ios = true
desktop = false
android = false
```

`nmp gen modules` produces `apps/podcast/nmp-app-podcast/` with the generated `AppAction`, `AppUpdate`, `ViewSpec`, capability trait composites, and UniFFI Swift bindings. Per ADR-0010 the Swift app sees:

```swift
enum AppAction {
    case kernel(KernelAction)
    case podcast(PodcastAction)             // from podcast-core
    case podcastLlm(PodcastLlmAction)
    case podcastRag(PodcastRagAction)
    case podcastFeeds(PodcastFeedsAction)
}
```

Generated property wrappers (`@PodcastLibrary`, `@NowPlaying`, etc.) come from the same codegen step — see [`wiring.md`](wiring.md).

---

## G. Tests

- Unit tests per DomainModule (round-trip serialize, index extraction).
- Per-ActionModule: `start()` validation table, `reduce()` capability-result handling table.
- One integration test per action chain (Download → Transcribe → Summarize → ExtractChapters) using `nmp-testing` mock capabilities.
- One golden-fixture test: feed XML in `tests/fixtures/timferriss.rss` → assertions on parsed `PodcastRecord` + `EpisodeRecord` count + first episode fields.

Per-crate test budget: ≤ 2,000 LOC of test code, no test file > 500 LOC.
