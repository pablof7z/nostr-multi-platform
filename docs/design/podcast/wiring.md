# Step 7 — wiring each Swift view

> Parent: [`../podcast-app-rebuild.md`](../podcast-app-rebuild.md). Companion to [`inventory.md`](inventory.md) and [`copy.md`](copy.md).

This doc is the per-view-group **wiring checklist**: which generated wrapper each Swift view consumes, which actions it dispatches, which sister views it depends on, in what order to wire them, and what to verify before the next group can start.

The work pattern, per group:

1. Generate the wrappers — `just gen-modules`.
2. Delete the matching `Bridge/Placeholders.swift` stubs.
3. Replace `// MARK: NMP-WIRE — TODO` blocks with `// MARK: NMP-WIRE — wired` consuming the generated `@<View>` property wrapper.
4. Drive the screenshot scenario for every screen in the group. Diff PNG must be empty.
5. Drive the user-flow XCUITest for every interaction in the group (subscribe → enqueue → play → mark-played etc.).
6. Commit; PR; pre-merge CI runs the diff harness.

---

## A. Generated-wrapper conventions

For each `ViewModule` named `FooViewModule`, codegen produces:

```swift
// Pseudo — actual output is per ADR-0010
@MainActor @Observable final class FooView {
    private(set) var payload: FooViewPayload?
    func dispatch(_ action: FooAction) { /* hops to actor */ }
}

@propertyWrapper
struct UseFoo: DynamicProperty {
    @StateObject private var live: FooView
    init(...spec args...) { _live = StateObject(wrappedValue: FooView(spec: ...)) }
    var wrappedValue: FooView { live }
}

func useFoo(...spec args...) -> FooView { /* hook-style alternative for free use */ }
```

Both styles are valid. Convention: use `@UseFoo` when the view's whole identity is the data; use `useFoo()` returning a `@StateObject` when the view has multiple data sources.

---

## B. Lane order (dependency-aware)

```
Lane 1 — Settings        (no deps)        [Singletons]
Lane 2 — Library         (deps: 1)        [Subscribe / list]
Lane 3 — Feed            (deps: 1, 2)     [Cross-podcast episodes]
Lane 4 — Player          (deps: 2, 3)     [AudioPlaybackCapability]
Lane 5 — Insights        (deps: 4)        [VoiceRecording + RAG]
Lane 6 — Ask             (deps: 4, 5)     [LLM + RAG]
Lane 7 — Components/Discover  (deps: all) [Discover sheet is huge — last]
```

Each lane is internally parallelisable (per-view) but lanes serialise.

---

## C. Per-group checklists

### C.1 Lane 1 — Settings (1 file, 168 LOC)

**`Views/Settings/SettingsView.swift`**

- Wrapper: `useSettings()`.
- Actions dispatched: `Podcast::UpdateSettings { patch }`, `Podcast::ClearImageCache`, `PodcastRag::ClearAllVectors`.
- Capability deps: `KeyValueStoreCapability` (lives behind `useSettings`; SettingsRecord is read-through).
- Special: each `Toggle($settings.allowCellularDownloads)` becomes a `.onChange` that calls `useSettings().dispatch(.updateSettings(patch:))` — keeps the SwiftUI binding shape while making the kernel the source of truth.
- Screenshot: `19-settings`.
- Acceptance: toggling Skip Ads on the rebuild flips the corresponding behavior in `AudioPlaybackOrchestrator` within one ViewBatch tick; UI reflects in < 100 ms.

### C.2 Lane 2 — Library (8 files, 1,930 LOC)

**Order** (within the lane, sequential):

1. `LibraryView.swift` — `useLibrary()` returns `LibraryViewModule`. Tap navigates to `PodcastDetailView`.
2. `PodcastDetailView.swift` — `usePodcastDetail(podcastId:)`. List of `EpisodeRow`s reused from Feed lane — placeholder until Lane 3.
3. `AddPodcastView.swift` — `useAddPodcastForm()`. Subscribe action.
4. `QueueView.swift` — `useProcessingQueue()`. Reads the kernel ledger.
5. `ActivityView.swift` — `useActivity(filter:)`. Reads `ActivityProjection`.
6. `DiscoverView.swift` — heavy. Pushed to Lane 7.
7. `PodcastDetailSheet.swift` — `usePodcastSheet(podcastIndexId:)`. Required by Lane 7.
8. `EpisodeDetailView.swift` — `useEpisodeDetail(episodeId:)`. Required by Lane 5.

- Wrappers: per [`inventory.md`](inventory.md) §G.5.
- Actions: `Podcast::Subscribe`, `Unsubscribe`, `RefreshFeed`, `RefreshAllFeeds`, `EnqueueDownload`, `CancelDownload`, `DeleteEpisode`.
- Screenshots: 03, 04, 07, 08.
- Acceptance: subscribe to a real RSS feed (Tim Ferriss) → library shows it → tap → see ≥ 5 episodes parsed; UI updates as `FetchFeed` action completes (no spinner gate; placeholder rows during fetch per doctrine D1).

### C.3 Lane 3 — Feed (2 files, 281 LOC)

**`Views/Feed/FeedView.swift`** → `useFeed()`. ContentUnavailableView when payload empty.
**`Views/Feed/EpisodeRow.swift`** → `useEpisodeRow(episodeId:)`. Used by Library and Feed.

- Actions: `Podcast::EnqueueDownload`, `Podcast::EnqueueTranscription`, `Podcast::Play`, `Podcast::Pause`, `Podcast::Resume`, `Podcast::DeleteEpisode`.
- Screenshots: 01, 02, 09.
- Acceptance: swipe-to-prioritize triggers ledger row reorder; UI reflects within one tick. Active-job badge on a row updates without re-rendering the whole list (per ADR-0001 composite keys).

### C.4 Lane 4 — Player (5 files, 1,647 LOC)

**Order**:

1. `MiniPlayer.swift` → `useNowPlaying()`. Drives the overlay in `ContentView.swift`.
2. `PlayerSheet.swift` → `usePlayerSheet()`. Multi-source: now_playing + chapters + guests + capture_state. Composes the three sub-sheets:
   - `ChaptersPanel.swift` → `useChapters(episodeId:)`.
   - `TranscriptView.swift` → `useTranscript(episodeId:)`.
   - `GuestAgentSheet.swift` → `useGuestAgent(guestId:, episodeId:)`. (Touches `podcast-llm` — defer the chat-on-tap behavior to Lane 6.)
3. `GuestAgentSheet.swift` chat input deferred to Lane 6.

- Actions: `Podcast::Play`, `Pause`, `Resume`, `Seek`, `SkipForward`, `SkipBack`, `SetRate`, `Stop`, `Insight::StartRecording`, `Insight::StopRecording`, `PodcastLlm::FindRelevantTimestamp`.
- Capability deps: `AudioPlaybackCapability` (Lane 4 cannot start until this lands), `LocalNotificationCapability` (lock-screen artwork already handled via `SetNowPlayingInfo`).
- Screenshots: 10, 11, 12, 13.
- Acceptance: play an episode → MiniPlayer appears → tap → PlayerSheet opens → seek works → background-audio survives lock screen; remote control center play/pause/skip all route through the kernel ledger; ad-skip behavior matches reference.

### C.5 Lane 5 — Insights (1 file, 252 LOC)

**`Views/Insights/InsightsView.swift`** → `useInsights()`.

- Actions: `Insight::DeleteInsight`, `Podcast::Play { from: excerpt_start_s }`, `Insight::PlayThoughtAudio` (UI-side — playing the thought recording with a transient `AVPlayer` is part of the Bridge layer; the Rust side knows nothing about the thought audio playback, only that it exists at `thought_audio_path`).
- Capability deps: `VoiceRecordingCapability` (for new insight capture via PlayerSheet — covered in Lane 4).
- Screenshots: 15, 16.
- Acceptance: capture an insight from PlayerSheet → it appears in InsightsView within one tick; tap "Listen to this part" → audio seeks to `excerpt_start_s`; delete with confirmation removes both the record and the audio file via `Insight::DeleteInsight` (cascade handled in Rust).

### C.6 Lane 6 — Ask (1 file, 322 LOC)

**`Views/Ask/AskView.swift`** → `useAsk()`.

Also wires the chat input of **`Views/Player/GuestAgentSheet.swift`** to `PodcastLlm::AskGuest`.

- Actions: `PodcastLlm::AskQuestion { session_id, query }`, `PodcastLlm::AskGuest { guest_id, episode_id, query }`.
- Streaming: token deltas arrive at ≤ 30 Hz; SwiftUI binding to `ChatTurnPayload.content` re-renders incrementally.
- Capability deps: `AppleIntelligenceCapability` (on iOS) or `rig.rs` provider with `KeyValueStoreCapability` for API keys.
- Screenshots: 17, 18.
- Acceptance: type a question → first token visible ≤ 1500 ms p99 on Wi-Fi (per M11 perf gate); citations chips render with correct episode/podcast titles + timestamps.

### C.7 Lane 7 — Components + Discover (2 files; the giant)

**`Views/Components/DiscoveryCards.swift`** is presentational only — no wiring beyond what the parents already provide.

**`Views/Library/DiscoverView.swift`** (898 LOC) is split mentally into 6 sub-view-modules. The Swift file shape stays one file (per copy step); each section reads from its own wrapper:

- Hero section → `useDiscoverHero()` reading `{ personalized_hero?, fallback_trending_lead }`.
- ForYou section → `useDiscoverForYou()` reading `{ recommendations: Vec<RecPayload> }` (conditional on `hasPersonalization`).
- Trending section → `useDiscoverTrending()` reading `{ podcasts: Vec<PodcastIndexPodcast> }`.
- Categories section → `useDiscoverCategories()` reading the static list — no kernel call.
- Topics section → `useDiscoverTopics()` reading `{ topics: Vec<String> }` (conditional).
- AddByURL section → static button → `useAddPodcastForm()` (from Lane 2).
- SearchResults section → `useDiscoverSearch(query:)` reading `{ podcasts, episodes }`.

`@StateObject` per sub-section is fine — each is independent and the parent layout doesn't need a synchronized payload.

- Actions: `PodcastFeeds::Search`, `PodcastFeeds::Trending`, `PodcastFeeds::PodcastsByCategory`, `PodcastRag::GetRecommendations`, `PodcastRag::GetPersonalizedHero`, `PodcastRag::GetUserTopics`, `Podcast::Subscribe`.
- Screenshots: 05, 06.
- Acceptance: trending populates within 2 s on first cold open; search-as-you-type debounced at 300 ms; personalized hero shows after subscribing to ≥ 1 podcast and playing ≥ 1 episode.

---

## D. Cross-cutting work

### D.1 `App/PodcastApp.swift` rewrite

The `@main` struct loses `ModelContainer` and gains:

```swift
@main
struct PodcastApp: App {
    @StateObject private var ffi = PodcastFfiApp.shared
    var body: some Scene {
        WindowGroup { ContentView() }
            .environmentObject(ffi)
    }
}

@MainActor final class PodcastFfiApp: ObservableObject {
    static let shared = PodcastFfiApp()
    let app: FfiApp
    init() {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        self.app = FfiApp(dataDir: dir.path, platform: "ios")
        // Register all bridges.
        app.setAudioPlaybackBridge(AudioPlaybackBridgeImpl())
        app.setHttpBridge(HttpBridgeImpl())
        app.setEmbeddingBridge(EmbeddingBridgeImpl())
        app.setTranscriptionBridge(TranscriptionBridgeImpl())
        app.setVoiceRecordingBridge(VoiceRecordingBridgeImpl())
        app.setAppleIntelligenceBridge(AppleIntelligenceBridgeImpl())
        app.setBackgroundWorkBridge(BackgroundWorkBridgeImpl())
        app.setLocalNotificationBridge(LocalNotificationBridgeImpl())
        app.setKeyValueStoreBridge(KeyValueStoreBridgeImpl())
        app.startListening()
    }
}
```

### D.2 `App/ContentView.swift` minimal change

Only the overlay condition changes from `audioService.currentEpisode != nil` to `nowPlaying.payload != nil` via `@UseNowPlaying`. The TabView and sheet structure are unchanged.

### D.3 Placeholder elimination

`grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` ratchets to zero across the 7 lanes.

---

## E. Per-lane PR template

```
title: m11: wire Lane <N> — <surface>

body:
- [ ] Generated wrappers refreshed (`just gen-modules`)
- [ ] All `// MARK: NMP-WIRE — TODO` in the lane removed
- [ ] Screenshots <ID list> pass diff
- [ ] Per-flow XCUITest in `ios/NmpPodcast/NmpPodcastUITests/Lane<N>_<surface>.swift` passes
- [ ] No new Swift business logic (grep verifies)
- [ ] No `nmp-core` patches in this PR (kernel must stay app-agnostic)
- [ ] Perf gate touched? <yes/no> — if yes, link `docs/perf/m11/<surface>.md`
```

---

## F. Anti-patterns to reject in review

- `if shouldRefresh { dispatch(.refresh) }` in a Swift view — the decision belongs in Rust; the view should always be allowed to call refresh, the action's `start()` decides validity.
- A `@State` variable holding a derived value (`var formattedDuration: String`) — must come from the payload.
- `Task { ... }` in a Swift view doing anything beyond invoking `dispatch(...)` — async I/O is Rust's job.
- New `Service` classes in Swift — none. Period.
- Calls to deleted Swift types — the placeholder `Placeholders.swift` is the only allowed staging file, and it must empty by M11 exit.
