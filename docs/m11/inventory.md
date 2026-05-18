# M11 Step 0 — File Inventory

> Created by task #41 (m11-step0-impl).
> Source: `/Users/pablofernandez/src/podcast/PodcastApp/` at task time.
> Parent spec: [`../design/podcast/inventory.md`](../design/podcast/inventory.md).

## A. Counts

| Area | Files | Notes |
|---|---:|---|
| Swift views (`ios/NmpPodcast/NmpPodcast/Views/`) | 29 | 20 original + 6 DiscoverView splits + 3 PlayerSheet splits |
| Swift bridge (`ios/NmpPodcast/NmpPodcast/Bridge/`) | 2 | ErrorPresentation.swift, Logger.swift |
| Rust crate sources (`apps/podcast/`) | 21 | 5 crates, type stubs only |

## B. Swift view files copied verbatim (29 files)

### Ask (1)

| Destination | Source | LOC | Notes |
|---|---|---:|---|
| `Views/Ask/AskView.swift` | `Views/Ask/AskView.swift` | 322 | soft-limit exception — no clean MARK seam |

### Components (2)

| Destination | Source | LOC | Notes |
|---|---|---:|---|
| `Views/Components/CachedAsyncImage.swift` | `Views/Components/CachedAsyncImage.swift` | 39 | |
| `Views/Components/DiscoveryCards.swift` | `Views/Components/DiscoveryCards.swift` | 302 | soft-limit exception |

### Feed (2)

| Destination | Source | LOC | Notes |
|---|---|---:|---|
| `Views/Feed/EpisodeRow.swift` | `Views/Feed/EpisodeRow.swift` | 230 | |
| `Views/Feed/FeedView.swift` | `Views/Feed/FeedView.swift` | 51 | |

### Insights (1)

| Destination | Source | LOC | Notes |
|---|---|---:|---|
| `Views/Insights/InsightsView.swift` | `Views/Insights/InsightsView.swift` | 252 | |

### Library (14 — includes 7 DiscoverView splits)

| Destination | Source | LOC | Notes |
|---|---|---:|---|
| `Views/Library/ActivityView.swift` | `Views/Library/ActivityView.swift` | 287 | verbatim |
| `Views/Library/AddPodcastView.swift` | `Views/Library/AddPodcastView.swift` | 89 | verbatim |
| `Views/Library/AllTrendingView.swift` | split from `Views/Library/DiscoverView.swift` | ~72 | MARK: All Trending View |
| `Views/Library/DiscoverCategoriesViews.swift` | split from `Views/Library/DiscoverView.swift` | ~130 | MARK: All Categories View + Category Detail View |
| `Views/Library/DiscoverSearchSupport.swift` | split from `Views/Library/DiscoverView.swift` | ~87 | MARK: Supporting Views |
| `Views/Library/DiscoverView.swift` | `Views/Library/DiscoverView.swift` | ~130 | coordinator only (main struct + body) |
| `Views/Library/DiscoverViewDataLoading.swift` | split from `Views/Library/DiscoverView.swift` | ~153 | MARK: Data Loading |
| `Views/Library/DiscoverViewSections.swift` | split from `Views/Library/DiscoverView.swift` | ~259 | MARK: Hero/ForYou/Trending/Categories/Topics/Search/AddByURL sections |
| `Views/Library/EpisodeDetailView.swift` | `Views/Library/EpisodeDetailView.swift` | 247 | verbatim |
| `Views/Library/LibraryView.swift` | `Views/Library/LibraryView.swift` | 120 | verbatim |
| `Views/Library/PodcastDetailSheet.swift` | `Views/Library/PodcastDetailSheet.swift` | 173 | verbatim |
| `Views/Library/PodcastDetailView.swift` | `Views/Library/PodcastDetailView.swift` | 45 | verbatim |
| `Views/Library/QueueView.swift` | `Views/Library/QueueView.swift` | 129 | verbatim |
| `Views/Library/TopicSearchView.swift` | split from `Views/Library/DiscoverView.swift` | ~68 | MARK: Topic Search View |

### Player (9 — includes 3 PlayerSheet splits)

| Destination | Source | LOC | Notes |
|---|---|---:|---|
| `Views/Player/ChaptersPanel.swift` | `Views/Player/ChaptersPanel.swift` | 324 | soft-limit exception |
| `Views/Player/GuestAgentSheet.swift` | `Views/Player/GuestAgentSheet.swift` | 297 | verbatim |
| `Views/Player/MiniPlayer.swift` | `Views/Player/MiniPlayer.swift` | 170 | verbatim |
| `Views/Player/PlayerSheet.swift` | `Views/Player/PlayerSheet.swift` | ~210 | core layout only |
| `Views/Player/PlayerSheetControls.swift` | split from `Views/Player/PlayerSheet.swift` | ~151 | MARK: Controls Bar + Capture Button + Toast Overlays |
| `Views/Player/PlayerSheetInsight.swift` | split from `Views/Player/PlayerSheet.swift` | ~181 | MARK: Gestures + Helpers + Insight Capture |
| `Views/Player/PlayerToasts.swift` | split from `Views/Player/PlayerSheet.swift` | ~34 | MARK: Toast Views |
| `Views/Player/TranscriptView.swift` | `Views/Player/TranscriptView.swift` | 214 | verbatim |

### Settings (1)

| Destination | Source | LOC | Notes |
|---|---|---:|---|
| `Views/Settings/SettingsView.swift` | `Views/Settings/SettingsView.swift` | 168 | verbatim |

## C. Swift bridge files (2)

| Destination | Source | Notes |
|---|---|---|
| `Bridge/ErrorPresentation.swift` | `Utilities/ErrorPresentation.swift` | UI-only error mapping |
| `Bridge/Logger.swift` | `Utilities/Logger.swift` | OSLog wrappers |

## D. Rust crates scaffolded (5)

All files are type-stub only. Zero business logic. Smoke tests pass.

| Crate | Path | Purpose |
|---|---|---|
| `podcast-core` | `apps/podcast/podcast-core/` | Domain records, action/view stubs |
| `podcast-feeds` | `apps/podcast/podcast-feeds/` | Feed parsing, Podcast Index client |
| `podcast-audio` | `apps/podcast/podcast-audio/` | AudioPlaybackCapability, NowPlaying |
| `podcast-rag` | `apps/podcast/podcast-rag/` | Vector store, embedding, retrieval |
| `podcast-llm` | `apps/podcast/podcast-llm/` | Dual-path LLM router, action stubs |

## E. Files NOT copied (remain Rust targets)

Per [`../design/podcast/inventory.md`](../design/podcast/inventory.md):

- `Models/*` (8 files) — become `DomainRecord`s in `podcast-core`
- `Services/*` (14 files) — become `ActionModule`s across all 5 crates
- `App/PodcastApp.swift`, `App/ContentView.swift`, `App/Config.swift` — rewritten for FFI
- `Utilities/RSSParser.swift` — replaced by `podcast-feeds::parser`
- `Services/ServiceContainer.swift` — deleted; kernel actor replaces it

## F. Split invariant

Both oversized files were split per `docs/design/podcast/copy.md §0a`:

- `DiscoverView.swift` (898 LOC → 7 files, all under 300 LOC)
- `PlayerSheet.swift` (642 LOC → 4 files, all under 220 LOC)

Content is byte-equivalent to the source (extensions access shared `@State` vars via Swift extension visibility rules).
