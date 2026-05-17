# Podcast rebuild — reference inventory

> Source: `/Users/pablofernandez/src/podcast/PodcastApp/`, commit at task time.
> Totals: **47 Swift files · 8,793 LOC**.
> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md).

## A. Inventory legend

- **Side**: `swift` = stays in `ios/NmpPodcast/Views/` verbatim (copy step only). `rust` = body deleted; replaced by Rust modules and FFI dispatch.
- **NMP host**: the extension crate that owns the moved logic.
- **Substrate kind**: which trait family in `nmp-core/src/substrate/` backs it.
- **Notes**: any non-mechanical decision recorded here so reviewers can verify.

The full Swift file path is `/Users/pablofernandez/src/podcast/PodcastApp/<path>` for every row.

## B. Entry points and shell

| Swift file | LOC | Role | Side | NMP host | Substrate kind | Notes |
|---|---:|---|---|---|---|---|
| `App/PodcastApp.swift` | 36 | `@main`; SwiftData container; injects `AudioService`, `ProcessingQueue` | swift | — | — | Rewritten to bind to `FfiApp` from `nmp-app-podcast` instead of `ModelContainer`. SwiftData schema deleted; persistence moves to Rust LMDB (post-M3) / sqlite-vec. |
| `App/ContentView.swift` | 63 | Root `TabView` (Feed / Ask / Insights / Library), MiniPlayer overlay, `PlayerSheet` sheet | swift | — | — | Reads `useNowPlaying()` for the overlay rendering decision; tabs unchanged structurally. |
| `App/Config.swift` | 35 | Podcast Index API key/secret env+plist | rust | `podcast-feeds` | — | Keys move to `KeyValueStoreCapability` (read-only on iOS; UserDefaults bridge). Validation is Rust-side bootstrap. |

## C. Models — all move to Rust

Every `@Model` final class becomes a `DomainRecord` inside a `DomainModule` in `podcast-core`. The SwiftData `@Relationship(deleteRule:)` semantics are reimplemented as Rust-side cascade in CRUD APIs (see [`podcast-core.md`](podcast-core.md) §B). The Swift `@Model` files are deleted from the new app — Rust types cross FFI via generated ViewModule payloads.

| Swift file | LOC | Rust type in `podcast-core::domain` | Cascade |
|---|---:|---|---|
| `Models/Podcast.swift` | 31 | `PodcastRecord` (id, feed_url, title, author, artwork_url, subscribed_at_ms) | episodes: cascade |
| `Models/Episode.swift` | 65 | `EpisodeRecord` (id, podcast_id, guid, title, ai_summary, description, audio_url, duration_s, published_at_ms, download_state, local_audio_path, playback_pos_s, has_been_played, transcript_id?, guest_ids[], insight_ids[]) | transcript: cascade · insights: cascade · guests: m:n (no cascade) |
| `Models/Transcript.swift` | 60 | `TranscriptRecord` (id, episode_id, full_text, language, generated_at_ms) + `TranscriptChunkRecord` (id, transcript_id, text, start_s, end_s, chunk_index, embedding[])  | chunks/chapters: cascade |
| `Models/Chapter.swift` | 34 | `ChapterRecord` (id, transcript_id, title, summary, start_s, end_s, chapter_index, is_ad) | leaf |
| `Models/Guest.swift` | 73 | `GuestRecord` + `GuestContentRecord` (source enum, text, url, published_at_ms, embedding[]) | enriched_content: cascade |
| `Models/Insight.swift` | 43 | `InsightRecord` (id, episode_id, created_at_ms, thought_text, thought_audio_path, excerpt_text, excerpt_start_s, excerpt_end_s, embedding[]) | leaf |
| `Models/Settings.swift` | 109 | `SettingsRecord` (one singleton per `IdentityScope::AppLocal`) — skip_forward_s, skip_backward_s, default_rate, allow_cellular, auto_transcribe, auto_summarize, auto_extract_chapters, default_summary_style, skip_ads | leaf; backed by `KeyValueStoreCapability` for live UI binding |
| `Models/AITypes.swift` | 26 | `SummaryStyle` enum + `ChatMessage` (kept in `podcast-llm`, not domain) | n/a |

## D. Services — all move to Rust

Each Swift service becomes an `ActionModule` (when it has side-effects) plus, where relevant, a `ProjectionCache` for derived data. The Swift `Services/` directory is **deleted** from the new app.

| Swift file | LOC | Becomes | NMP host | Capability dep |
|---|---:|---|---|---|
| `Services/AIService.swift` | 308 | `SummarizeEpisode`, `ExtractChapters`, `FindRelevantTimestamp` actions + the prompt library | `podcast-llm` | `AppleIntelligenceCapability` (iOS) or `rig.rs` (fallback) |
| `Services/AudioService.swift` | 323 | `Play`, `Pause`, `Resume`, `Seek`, `SkipForward`, `SkipBack`, `SetRate`, `Stop` actions + `NowPlayingViewModule` + ad-skip policy | `podcast-core` | `AudioPlaybackCapability` |
| `Services/DownloadService.swift` | 150 | `DownloadEpisode`, `CancelDownload`, `DeleteDownload` actions + per-episode progress projection | `podcast-core` | `HttpCapability` (streaming + background) |
| `Services/GuestEnrichmentService.swift` | 94 | `IdentifyGuests`, `EnrichGuest` actions; uses transcript context | `podcast-llm` | `AppleIntelligenceCapability` |
| `Services/ImageCache.swift` | 87 | Stays Swift (UI-side cache wrapping a generic `ImageCapability` if needed) | swift in `ios/NmpPodcast/Bridge/ImageCache.swift` | none |
| `Services/InsightService.swift` | 233 | `StartInsightRecording`, `StopInsightRecording`, `MatchExcerpt` actions + `InsightsViewModule` | `podcast-core` + `podcast-llm` (matchExcerpt) | `VoiceRecordingCapability`, `TranscriptionCapability`, `EmbeddingCapability` |
| `Services/PodcastIndexService.swift` | 186 | `SearchPodcasts`, `TrendingPodcasts`, `PodcastsByCategory` actions (HMAC-SHA1 auth) | `podcast-feeds` | `HttpCapability` |
| `Services/PodcastService.swift` | 118 | `FetchFeed`, `RefreshFeed`, `RefreshAllFeeds` actions | `podcast-feeds` | `HttpCapability` |
| `Services/ProcessingQueue.swift` | 360 | The action ledger — already in `nmp-core::kernel::ledger`. Per-domain action chain (Download → Transcribe → Summarize → ExtractChapters) becomes a state machine in `podcast-core` orchestrator. Per-job statuses are kernel ledger rows. | `podcast-core` orchestrator | all of the above |
| `Services/RAGService.swift` | 257 | `Index`, `Retrieve`, `Chat` actions (`Chat` is `podcast-llm::AskQuestion` + this `Retrieve`) | `podcast-rag` | `EmbeddingCapability` |
| `Services/RecommendationService.swift` | 235 | `GetRecommendations`, `GetPersonalizedHero`, `GetUserTopics`, `SearchEpisodes` actions | `podcast-rag` + `podcast-feeds` | `EmbeddingCapability`, `HttpCapability` |
| `Services/ServiceContainer.swift` | 54 | Deleted. ServiceContainer is the kernel actor. | — | n/a |
| `Services/TranscriptionService.swift` | 222 | `Transcribe` action; chunking heuristics in Rust | `podcast-core` (orchestration) + `TranscriptionCapability` (iOS exec) | `TranscriptionCapability` |
| `Services/VectorDatabase.swift` | 259 | Reimplemented as the storage layer of `podcast-rag` (sqlite-vec via Rust `sqlite-vec` crate) | `podcast-rag` | none (uses LMDB-companion sqlite store) |

## E. Utilities — mixed

| Swift file | LOC | Side | NMP host | Notes |
|---|---:|---|---|---|
| `Utilities/RSSParser.swift` | 261 | rust | `podcast-feeds` | Replaced by `feed-rs` or `rss` Rust crates plus a Podcasting 2.0 extension parser. The Swift class is deleted. |
| `Utilities/ErrorPresentation.swift` | 58 | swift | `ios/NmpPodcast/Bridge/ErrorPresentation.swift` | UI-only — `AppError`/`ErrorHandler` map `toast: Option<String>` from `AppState` (doctrine D3). No business logic. |
| `Utilities/Logger.swift` | 14 | swift | `ios/NmpPodcast/Bridge/Logger.swift` | UI-side OSLog wrappers. Rust uses `tracing`. |

## F. ViewModels

The Swift `PodcastApp/ViewModels/` directory exists but is **empty in the canonical app at the task-time commit**. The "view model" role is filled by `@Observable` services (AudioService, InsightService, ProcessingQueue, Settings). All of those become Rust ViewModules in the rebuild.

## G. Views (UI) — all stay Swift, verbatim copy

Each `.swift` view file is `cp -R`'d into `ios/NmpPodcast/Views/` with the same relative path. Bodies are unchanged except where they read from a `@Bindable var service: AudioService` etc.; those references become `@StateObject var live = useNowPlaying()` (etc.) — generated property wrappers from [`wiring.md`](wiring.md). The file shape, fonts, colors, padding, gesture handlers, `accessibilityIdentifier`s — all unchanged.

### G.1 Ask (1 file)

| Swift file | LOC | NMP wrapper | Reads | Dispatches |
|---|---:|---|---|---|
| `Views/Ask/AskView.swift` | 322 | `useAsk()` | `AskViewModule { messages: [ChatTurn], suggested: [String], session_id, episode_count, hours_listened }` | `PodcastLlm::AskQuestion { session_id, query }`, `Rag::Index{...}` lazy on first ask |

### G.2 Components (2 files)

| Swift file | LOC | NMP wrapper | Notes |
|---|---:|---|---|
| `Views/Components/CachedAsyncImage.swift` | 39 | none (UI-only) | Image cache lives in Swift, backed by URLSession (NMP is unopinionated about UI-side caches per ADR-0005 §native shadow). |
| `Views/Components/DiscoveryCards.swift` | 302 | none (presentational) | Takes typed records from the Discover view module as input. No business logic to migrate. |

### G.3 Feed (2 files)

| Swift file | LOC | NMP wrapper | Reads | Dispatches |
|---|---:|---|---|---|
| `Views/Feed/FeedView.swift` | 51 | `useFeed()` | `FeedViewModule { episodes: [EpisodeRowPayload] }` (sorted desc by published_at_ms) | `Podcast::EnqueueDownload { episode_id }` (swipe-to-prioritize), `Podcast::DeleteEpisode { episode_id }` (swipe-to-delete) |
| `Views/Feed/EpisodeRow.swift` | 230 | `useEpisodeRow(episode_id)` | `EpisodeRowViewModule` (per-row payload: title, podcast_title, summary, duration, download_state, active_job_kind?, has_insights, is_playing) | `Podcast::Play`, `Podcast::Pause`, `Podcast::Resume`, `Podcast::EnqueueDownload`, `Podcast::EnqueueTranscription` |

### G.4 Insights (1 file)

| Swift file | LOC | NMP wrapper | Reads | Dispatches |
|---|---:|---|---|---|
| `Views/Insights/InsightsView.swift` | 252 | `useInsights()` | `InsightsViewModule { cards: [InsightCardPayload] }` | `Podcast::DeleteInsight`, `Podcast::Play { from: excerpt_start_s }` |

### G.5 Library (8 files)

| Swift file | LOC | NMP wrapper | Reads | Dispatches |
|---|---:|---|---|---|
| `Views/Library/ActivityView.swift` | 287 | `useActivity()` | `ActivityViewModule { stats, episodes: [EpisodeStatusRow] }` (filter-aware) | `ProcessingQueue::CancelJob` |
| `Views/Library/AddPodcastView.swift` | 89 | `useAddPodcastForm()` | local state | `PodcastFeeds::FetchFeed { url }` → on success `Podcast::Subscribe { podcast_id }` |
| `Views/Library/DiscoverView.swift` | 130 | `useDiscover(search_text)` | `DiscoverViewModule { hero, recommendations, trending, categories, topics, search_results }` (split into nested view modules — see [`wiring.md`](wiring.md) §Library) | `PodcastFeeds::Search`, `PodcastRag::GetRecommendations`, `Podcast::Subscribe`, etc. |
| `Views/Library/DiscoverViewSections.swift` | ~259 | (extension on `DiscoverView`) | all 7 `@ViewBuilder` section vars (Hero, ForYou, Trending, Categories, Topics, AddByURL, SearchResults) | — |
| `Views/Library/DiscoverViewDataLoading.swift` | ~153 | (extension on `DiscoverView`) | data-loading methods (loadTrending, loadRecommendations, etc.) | — |
| `Views/Library/DiscoverSearchSupport.swift` | ~87 | none | `PodcastSearchRow`, `EpisodeSearchRow` presentational cells | — |
| `Views/Library/AllTrendingView.swift` | ~72 | `useDiscoverTrending()` | `AllTrendingView` detail screen | — |
| `Views/Library/DiscoverCategoriesViews.swift` | ~130 | none | `AllCategoriesView`, `CategoryDetailView` detail screens | — |
| `Views/Library/TopicSearchView.swift` | ~68 | none | `TopicSearchView` detail screen | — |
| `Views/Library/EpisodeDetailView.swift` | 247 | `useEpisodeDetail(episode_id)` | `EpisodeDetailViewModule { header, summary?, description?, insights: [InsightCardPayload] }` | `Podcast::Play`, `Podcast::DeleteInsight` |
| `Views/Library/LibraryView.swift` | 120 | `useLibrary()` | `LibraryViewModule { podcasts: [PodcastRowPayload] }` | `Podcast::Unsubscribe`, `Podcast::RefreshAllFeeds` |
| `Views/Library/PodcastDetailSheet.swift` | 173 | `usePodcastSheet(podcast_index_id)` | `PodcastSheetViewModule { metadata, is_already_subscribed }` | `Podcast::Subscribe` |
| `Views/Library/PodcastDetailView.swift` | 45 | `usePodcastDetail(podcast_id)` | `PodcastDetailViewModule { podcast, episodes_desc }` | `Podcast::RefreshFeed { podcast_id }` |
| `Views/Library/QueueView.swift` | 129 | `useProcessingQueue()` | `ProcessingQueueViewModule { active: [QueueJobRow], completed: [...], failed: [...] }` (reads directly from the kernel action ledger) | `Ledger::CancelAction`, `Ledger::ClearCompleted`, `Ledger::ClearFailed` |

### G.6 Player (5 files)

| Swift file | LOC | NMP wrapper | Reads | Dispatches |
|---|---:|---|---|---|
| `Views/Player/MiniPlayer.swift` | 170 | `useNowPlaying()` | `NowPlayingViewModule { episode_id, podcast_id, title, podcast_title, artwork_url, progress_pct, state }` | `Podcast::Pause`, `Podcast::Resume`, `Podcast::SkipForward`, `Podcast::SkipBack`, `Podcast::Seek` |
| `Views/Player/PlayerSheet.swift` | ~210 | `usePlayerSheet()` | `PlayerSheetViewModule { episode, summary?, chapters, guests, capture_state }` | `Podcast::Seek`, `Podcast::SetRate`, `Insight::StartRecording`, `Insight::StopRecording { episode_id, capture_time_s }`, opens `ChaptersPanel`, `TranscriptView`, `GuestAgentSheet` |
| `Views/Player/PlayerSheetControls.swift` | ~151 | (extension on `PlayerSheet`) | Controls Bar, Capture Button, Toast Overlays | — |
| `Views/Player/PlayerSheetInsight.swift` | ~181 | (extension on `PlayerSheet`) | Gestures, Helpers, Insight Capture | — |
| `Views/Player/PlayerToasts.swift` | ~34 | none | `InsightErrorToast`, `InsightSavedToast` | — |
| `Views/Player/ChaptersPanel.swift` | 324 | `useChapters(episode_id)` | `ChaptersViewModule { chapters, current_index?, is_extracting }` | `Podcast::Seek { to: chapter.start_s }`, `PodcastLlm::FindRelevantTimestamp { query }` |
| `Views/Player/GuestAgentSheet.swift` | 297 | `useGuestAgent(guest_id, episode_id)` | `GuestAgentViewModule { guest, messages, suggested: [...] }` | `PodcastLlm::EnrichGuest`, `PodcastLlm::AskGuest { guest_id, query }` |
| `Views/Player/TranscriptView.swift` | 214 | `useTranscript(episode_id)` | `TranscriptViewModule { chunks, current_chunk?, is_transcribing, is_summarizing, summary? }` | `Podcast::Seek { to: chunk.start_s }`, `Podcast::EnqueueTranscription` |

### G.7 Settings (1 file)

| Swift file | LOC | NMP wrapper | Reads | Dispatches |
|---|---:|---|---|---|
| `Views/Settings/SettingsView.swift` | 168 | `useSettings()` | `SettingsViewModule { settings: SettingsRecord, version, build }` | `Podcast::UpdateSettings { ... }`, `Podcast::ClearImageCache`, `PodcastRag::ClearVectors` |

## H. Counts

- **Swift files staying Swift (UI):** 20 (`Views/*.swift`; verified by `find /Users/pablofernandez/src/podcast/PodcastApp/Views -name '*.swift' | wc -l` = 20) + 3 utility/shell (`PodcastApp.swift`, `ContentView.swift`, two `Bridge/*.swift` ports).
- **Swift files moving to Rust:** 8 models + 14 services + 1 utility (`RSSParser`) + 1 config = **24 files / ~3,165 LOC** moving to Rust.
- **Swift files deleted entirely (no replacement needed):** `Services/ServiceContainer.swift` (54 LOC; replaced by the kernel actor + generated wrappers).

## I. Verification protocol

After Step 0 (copy + split):
- `find ios/NmpPodcast/Views -name '*.swift' | wc -l` = 29 (20 original + 6 DiscoverView siblings + 3 PlayerSheet siblings; `AskView.swift` 322, `DiscoveryCards.swift` 302, `ChaptersPanel.swift` 324 are soft-limit exceptions; see [`copy.md`](copy.md) §0a).
- `find ios/NmpPodcast -path '*/Models/*' -o -path '*/Services/*' -o -path '*/ViewModels/*'` = 0 hits.
- `grep -RnE 'import SwiftData' ios/NmpPodcast` = 0 hits (SwiftData replaced by LMDB/Rust persistence).
