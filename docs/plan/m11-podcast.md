# M11 — Podcast app (the `../podcast` rebuild on NMP — the kernel-boundary proof)

> Part of the [Build & Validation Plan](../plan.md). Arc 2 — kernel-boundary proof with a non-social-domain app.

**Demo product:** A 1:1 rebuild of `/Users/pablofernandez/src/podcast` (the fully-functional Swift app, 20 SwiftUI views, ~8.8k LOC of Swift) running on NMP. **UI is pixel-identical** to the reference Swift app; **all business logic, LLM, audio orchestration, downloads, transcripts, RAG, recommendations** are in Rust extension modules driving the kernel.

**This is the load-bearing kernel-boundary check.** If the kernel needs even one podcast noun to make this work, the boundary is wrong and we go back to fix it.

**Reference inputs** (read before scoping):

- `/Users/pablofernandez/src/podcast/` — canonical Swift implementation. Source of truth for UI and feature behavior. **Every view in `PodcastApp/Views/` is copied verbatim into `ios/NmpPodcast/Views/`** as step 1; only the data source is rewired.
- `/Users/pablofernandez/src/podcast-rmp/` — prior WIP RMP rewrite (incomplete). **Not a code source** but a lessons source: read its `RMP-ARCHITECTURE-BIBLE.md`, `FINAL_PLAN.md`, `docs/plans/iphone-feature-parity-plan.md`, and `docs/plans/iphone-feature-parity-checklist.md` before scoping. That repo's `AGENTS.md` is the working guide for any agent touching that tree.
- `/Users/pablofernandez/src/podcast/docs/plans/` — original feature design docs (podcast-app-design, discovery-tab-redesign, insights-feature-design).

**Reference inventory of the Swift app** (so the scope is explicit, not vibes):

| Swift `Views/` group | Files | NMP target |
|---|---|---|
| `Ask/` | AskView.swift | `ask-core` ActionModule + ViewModule wrapping `rig.rs` LLM call |
| `Components/` | CachedAsyncImage, DiscoveryCards | reusable Swift components, ported as-is; image cache backed by NMP Blossom-aware capability |
| `Feed/` | FeedView, EpisodeRow | `podcast-core::FeedViewModule` + `EpisodeRowViewModule` |
| `Insights/` | InsightsView | `insights-core` ViewModule + ActionModule (uses RAG via `rig.rs`) |
| `Library/` | ActivityView, AddPodcastView, DiscoverView, EpisodeDetailView, LibraryView, PodcastDetailSheet, PodcastDetailView, QueueView | `podcast-core` ViewModules + ActionModules |
| `Player/` | ChaptersPanel, GuestAgentSheet, MiniPlayer, PlayerSheet, TranscriptView | `player-core` ViewModule + `AudioPlaybackCapability` |
| `Settings/` | SettingsView | `settings-core` ActionModule (mostly capability invocations) |

Swift `Services/` (AIService, AudioService, DownloadService, GuestEnrichmentService, ImageCache, InsightService, PodcastIndexService, PodcastService, ProcessingQueue, RAGService, RecommendationService, ServiceContainer, TranscriptionService, VectorDatabase) **all move to Rust** as ActionModules + ProjectionCaches + capability bridges; Swift loses its Services/ directory entirely.

Swift `Models/` (AITypes, Chapter, Episode, Guest, Insight, Podcast, Settings, Transcript) **all move to Rust** as DomainRecords inside `podcast-core` and sibling crates.

Swift `ViewModels/` **disappear** — they become Rust ViewModules whose output crosses FFI as typed ViewBatch deltas.

**Scope.**

**Step 0 — copy step (UI-fidelity invariant lock):**

- Copy every file in `/Users/pablofernandez/src/podcast/PodcastApp/Views/` into `ios/NmpPodcast/NmpPodcast/Views/` verbatim. Commit immediately. No edits except the minimum needed to compile against placeholder data sources (`// MARK: NMP-WIRE` markers).
- Copy `Resources/Assets.xcassets` and `Info.plist` (sanitized) verbatim.
- The result compiles and renders against stubbed data; UI is visually identical to `../podcast` per a side-by-side simulator screenshot diff (≤ 1 px tolerance, font-rendering exceptions documented).

**Step 1 — domain + view modules in Rust** (per the table above):

- `apps/podcast/podcast-core/` — main app crate. `DomainModule`s: `Podcast`, `Episode`, `Transcript`, `Chapter`, `Guest`, `Insight`, `Subscription`, `PlayerState`, `QueueEntry`, `Activity`.
- `apps/podcast/podcast-core/` — `ViewModule`s: `PodcastLibrary`, `EpisodeDetail`, `NowPlaying`, `EpisodeQueue`, `Discover`, `Insights`, `Activity`, `PodcastDetail`, `Feed`, `EpisodeRow`, `Chapters`, `Transcript`, `MiniPlayer`, `PlayerSheet`, `GuestAgent`, `Ask`, `Settings`.
- `apps/podcast/podcast-core/` — `ActionModule`s: `SubscribePodcast`, `UnsubscribePodcast`, `RefreshFeed`, `DownloadEpisode`, `CancelDownload`, `Play`, `Pause`, `Seek`, `SkipForward`, `SkipBack`, `MarkPlayed`, `EnqueueEpisode`, `ReorderQueue`, `ImportRss`, `ImportOpml`, `AskQuestion`, `EnrichGuest`, `RunInsight`, `SearchPodcasts`.
- `apps/podcast/podcast-llm/` — LLM-driven actions via `rig.rs`: `AskQuestion`, `EnrichGuest`, `RunInsight`. Uses the kernel's capability bridge for HTTP + key storage.
- `apps/podcast/podcast-rag/` — RAG + vector DB store; uses a swappable `EmbeddingCapability` and a Rust-side vector store (sqlite-vss or qdrant-client).
- `apps/podcast/podcast-feeds/` — RSS + Atom + JSON Feed + Podcast 2.0 namespaces parsing; transcripts; chapters; value-for-value. Pure Rust; pulls via `HttpCapability`.

**Step 2 — capabilities added to the kernel's reusable set** (these are general, not podcast-specific):

- `AudioPlaybackCapability`: play URL or local file; report position events + state transitions back; iOS impl via `AVPlayer` + background-audio entitlement + lock-screen `MPNowPlayingInfoCenter`/`MPRemoteCommandCenter`.
- `BackgroundWorkCapability`: register periodic background tasks; iOS impl via `BGTaskScheduler`.
- `LocalNotificationCapability`: extended for episode-available alerts.
- `HttpCapability`: long-running streaming response support (RSS, transcripts).
- `EmbeddingCapability`: callable embedding model; kernel-owned policy, platform-owned execution (CoreML on iOS, ONNX or remote API as fallback).
- `KeyValueStoreCapability`: typed persistent KV (for saved playback position when persistence-by-store is overkill).

**Step 3 — protocol module integration:**

- `nmp-podcast` is **not a v1 deliverable** unless a real published NIP is selected during M11 design (e.g. NIP-54 or a successor) and the choice is recorded in `docs/design/podcast-app-rebuild.md`. Until then, the podcast app uses RSS + Podcast 2.0 namespaces (chapters, transcripts, value-for-value) via `podcast-feeds`, and Nostr is the **social overlay only** — kind:1 discussion threads referencing the episode URL/GUID, kind:7 reactions. (NIP-57 zaps are post-v1 per [post-v1.md](post-v1.md) and are not in scope for M11.) The decision is locked in M11 step-0; no `NIP-XX` placeholders allowed in code or in plans past that point.

**Step 4 — wire each copied Swift view to its Rust view module:**

- Replace stubbed data with the generated wrapper hooks (`@PodcastLibrary`, `@NowPlaying`, etc. — produced by `nmp gen modules`).
- The Swift file shape stays the same; only the data source changes.
- After every Library/Feed/Player/Insights/Ask/Settings group is wired, run the side-by-side screenshot diff again.

**Exit gate (kernel boundary).**

- **`nmp-core` gains zero podcast nouns.** No `Podcast`, `Episode`, `Transcript`, `Chapter`, `Player`, `Feed`, `Insight`, `Guest` types added to the kernel. Verified by grep + manual review at the commit.
- **The capability families added in M11 are general** (audio playback, background work, local notifications, HTTP, embedding, KV-store). Their request/response shapes are not podcast-specific.
- **Reactivity behavior is identical** to the Twitter slice — composite-key dependencies, delta coalescing, claim-based GC, ADR-0007 diagnostics all work for podcast view modules.
- **No app-state leaks across the boundary in either direction:** no Nostr type appears in `podcast-core`'s public surface; no podcast type appears in `nmp-core`'s public surface.

**Exit gate (product fidelity to `../podcast`).**

- **UI parity:** side-by-side screenshot of every screen in `../podcast` vs `ios/NmpPodcast` matches at ≤ 1 px tolerance (font/rendering differences whitelisted explicitly in `docs/perf/m11/parity-screenshots.md`).
- **Feature parity:** every user flow exercised in `/Users/pablofernandez/src/podcast/Tests/` (or its equivalent on the canonical Swift app) reproduced as a scripted Sonnet-agent run on `ios/NmpPodcast`. No "feature dropped" footnotes.
- **Subscribe to 10 real podcasts** spanning RSS + (where available) Nostr feeds; library populates correctly.
- **Download an episode in the background** while the app is suspended; resumable on relaunch.
- **Play with background audio** while the iPhone is locked; lock-screen artwork, scrubber, skip/seek controls all functional.
- **Resume playback at the correct position** after a kill-relaunch.
- **Push notification on a new episode arrival.**
- **Ask a question** about an episode; answer streams in via `rig.rs` LLM with the transcript as RAG context.
- **Insights** view generates a structured episode summary on demand.
- **Guest enrichment** populates guest cards via external lookup, identical to the Swift impl behavior.

**Stress + perf gates.**

- Library of 100 podcasts × 50 episodes (5k episodes total) scrolls at 60 fps on iPhone 12.
- Player UI updates every 250 ms during playback without visible jank.
- Download queue with 20 concurrent downloads keeps the UI responsive.
- LLM ask flow streams first token in ≤ 1500 ms over Wi-Fi; full answer in ≤ 8 s for an average-length episode (measured).
- Battery drain during 1 hour of background playback ≤ Swift baseline + 10 %.

**Runnable artifact.** `ios/NmpPodcast` — distinct binary, same Rust kernel, different module set, **same UI as `../podcast`**. Report in `docs/perf/m11/podcast-app.md` documenting kernel-boundary verification, parity screenshots, and the perf measurements above.
